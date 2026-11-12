using System.Diagnostics;
using System.Threading.Channels;

namespace Pmac;

public sealed class Runtime : IDisposable, IAsyncDisposable
{
    private readonly object gate = new();
    private readonly CompletionPump completions;
    private readonly List<WeakReference<Client>> clients = new();
    private ulong id;
    private bool closing;

    public Runtime(uint workerThreads = 2, int completionCapacity = 1024)
    {
        if (completionCapacity < 1) throw new ArgumentOutOfRangeException(nameof(completionCapacity));
        if (NativeMethods.pmac_abi_version() / 1000 != 2) throw new PmacException(-25, "native library ABI major is incompatible with this SDK");
        var config = new NativeMethods.RuntimeConfig { StructSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf<NativeMethods.RuntimeConfig>(), AbiVersion = NativeMethods.AbiVersion, WorkerThreads = workerThreads };
        Check(NativeMethods.pmac_runtime_create(ref config, out id));
        completions = new CompletionPump(completionCapacity, id);
    }

    ~Runtime()
    {
        var value = Volatile.Read(ref id);
        if (value == 0) return;
        // Finalizers never wait for network or background work.
        try
        {
            completions.Stop();
            if (NativeMethods.pmac_runtime_shutdown(value, 0) == 0) NativeMethods.pmac_runtime_release(value);
        }
        catch { }
    }

    public static uint NativeAbiVersion => NativeMethods.pmac_abi_version();
    public static string NativeVersion => System.Runtime.InteropServices.Marshal.PtrToStringUTF8(NativeMethods.pmac_version_string()) ?? "";

    public static uint NativeVersionNumber => NativeMethods.pmac_version_number();
    public static RuntimeInfo NativeInfo
    {
        get
        {
            Check(NativeMethods.pmac_get_runtime_info(out var info));
            return new RuntimeInfo(info.AbiMajor, info.AbiMinor, info.VersionNumber, info.FeatureFlags, info.PointerWidth);
        }
    }
    public Client CreateClient(uint maxRequests = 64, nuint maxRetainedBytes = 80 * 1024 * 1024, uint maxFileTasks = 1)
    {
        lock (gate)
        {
            ThrowIfClosing();
            var config = new NativeMethods.ClientConfig { StructSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf<NativeMethods.ClientConfig>(), AbiVersion = NativeMethods.AbiVersion, MaxRequests = maxRequests, MaxRetainedBytes = maxRetainedBytes, MaxFileTasks = maxFileTasks };
            Check(NativeMethods.pmac_client_create(id, ref config, out var clientId));
            var client = new Client(clientId, this);
            clients.RemoveAll(reference => !reference.TryGetTarget(out _));
            clients.Add(new WeakReference<Client>(client));
            return client;
        }
    }

    internal Request Submit(Func<ulong> submit)
    {
        lock (gate)
        {
            ThrowIfClosing();
            completions.Reserve();
            Request? request = null;
            try
            {
                request = new Request(submit(), this);
                completions.TrackReserved(request);
                return request;
            }
            catch
            {
                request?.Abandon();
                completions.ReleaseSlot();
                throw;
            }
        }
    }
    private void ThrowIfClosing()
    {
        if (id == 0) throw new ObjectDisposedException(nameof(Runtime));
        if (closing) throw new PmacException(-13, "runtime is closing");
    }

    public void Shutdown(uint waitMs = 1000)
    {
        var timer = Stopwatch.StartNew();
        lock (gate)
        {
            if (id == 0) return;
            closing = true;
            // A pending shutdown retains the native ID and can be retried.
            Check(NativeMethods.pmac_runtime_shutdown(id, waitMs));
            completions.Stop();
            var remaining = Math.Max(0, (long)waitMs - timer.ElapsedMilliseconds);
            if (!completions.Task.Wait(TimeSpan.FromMilliseconds(remaining)))
                throw new PmacException(-14, "runtime completion pump is still stopping");
            foreach (var reference in clients)
                if (reference.TryGetTarget(out var client)) client.ReleaseAfterRuntimeShutdown();
            Check(NativeMethods.pmac_runtime_release(id));
            Volatile.Write(ref id, 0UL);
            GC.SuppressFinalize(this);
        }
    }

    public void Dispose() => Shutdown();
    public async ValueTask DisposeAsync() => await Task.Run(() => Shutdown()).ConfigureAwait(false);

    internal static void Check(int status)
    {
        if (status != 0) throw new PmacException(status, $"PMAC runtime/client operation failed with status {status}");
    }
}

// One blocking completion thread per Runtime; no per-request waiting threads.
// This object never captures Runtime, so an idle pump cannot retain its owner.
internal sealed class CompletionPump
{
    private readonly Channel<WeakReference<Request>> queue;
    private readonly SemaphoreSlim slots;
    private readonly AutoResetEvent fallbackWake = new(false);
    private readonly ulong runtimeId;
    private readonly bool nativeNotifications;
    private int stopping;
    internal Task Task { get; }

    internal CompletionPump(int capacity, ulong runtimeId)
    {
        this.runtimeId = runtimeId;
        queue = Channel.CreateBounded<WeakReference<Request>>(new BoundedChannelOptions(capacity) { SingleReader = true });
        slots = new SemaphoreSlim(capacity, capacity);
        try { nativeNotifications = NativeMethods.pmac_runtime_wake(runtimeId) == 0; }
        catch (EntryPointNotFoundException) { nativeNotifications = false; }
        Task = System.Threading.Tasks.Task.Factory.StartNew(Run, CancellationToken.None,
            TaskCreationOptions.LongRunning, TaskScheduler.Default);
    }

    internal void Reserve()
    {
        if (!slots.Wait(0)) throw new PmacException(-3, "runtime completion capacity is full before submission");
    }

    internal void ReleaseSlot() => slots.Release();

    internal void TrackReserved(Request request)
    {
        if (!queue.Writer.TryWrite(new WeakReference<Request>(request)))
            throw new PmacException(-13, "runtime completion pump is closing");
        Wake();
    }

    private void Wake()
    {
        if (nativeNotifications) NativeMethods.pmac_runtime_wake(runtimeId);
        else fallbackWake.Set();
    }

    internal void Stop()
    {
        if (Interlocked.Exchange(ref stopping, 1) != 0) return;
        queue.Writer.TryComplete();
        Wake();
    }

    private void Run()
    {
        var pending = new List<WeakReference<Request>>();
        ulong cursor = 0;
        try
        {
            while (true)
            {
                while (queue.Reader.TryRead(out var request)) pending.Add(request);
                for (var index = pending.Count - 1; index >= 0; --index)
                {
                    if (pending[index].TryGetTarget(out var tracked))
                    {
                        var status = tracked.Poll(0, out _);
                        tracked = null;
                        if (status == -6) continue;
                    }
                    pending.RemoveAt(index);
                    slots.Release();
                }
                if (Volatile.Read(ref stopping) != 0 && pending.Count == 0) return;
                if (nativeNotifications)
                {
                    var status = NativeMethods.pmac_runtime_wait_completion(runtimeId, ref cursor, 100);
                    if (status != 0 && status != -6 && Volatile.Read(ref stopping) == 0)
                        throw new PmacException(status, "runtime completion notification failed");
                }
                else fallbackWake.WaitOne(pending.Count == 0 ? Timeout.Infinite : 1);
            }
        }
        catch (Exception error)
        {
            Interlocked.Exchange(ref stopping, 1);
            queue.Writer.TryComplete();
            while (queue.Reader.TryRead(out var request)) pending.Add(request);
            foreach (var reference in pending)
            {
                if (reference.TryGetTarget(out var request)) request.FailPump(error);
                slots.Release();
            }
        }
        finally { fallbackWake.Dispose(); }
    }
}
