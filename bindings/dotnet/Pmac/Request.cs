using System.Runtime.InteropServices;

namespace Pmac;

public sealed class Request : IDisposable
{
    private readonly object gate = new();
    private readonly TaskCompletionSource<RequestInfo> completion = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private ulong id;
    private bool disposed;
    private bool completed;
    private RequestInfo? completedInfo;
    private PmacException? completedError;

    internal Request(ulong id, Runtime runtime) { this.id = id; Runtime = runtime; }
    internal Runtime Runtime { get; }
    internal Task<RequestInfo> Completion => completion.Task;
    public ulong Id { get { lock (gate) return CheckedId(); } }

    ~Request()
    {
        try { Abandon(); } catch { }
    }

    private ulong CheckedId()
    {
        if (disposed || id == 0) throw new ObjectDisposedException(nameof(Request));
        return id;
    }

    public RequestRoute Route
    {
        get
        {
            lock (gate)
            {
                NativeCheck(NativeMethods.pmac_request_get_route(CheckedId(), out var memberId, out var epoch));
                return new RequestRoute(memberId, epoch);
            }
        }
    }
    public RequestInfo Wait(uint waitMs = uint.MaxValue)
    {
        var requestId = Id;
        var status = NativeMethods.pmac_request_wait(requestId, waitMs, out var info);
        if (status == -6) throw new PmacException(status, "request wait budget expired", requestId);
        lock (gate)
        {
            if (!completed) Finish(info, status);
            if (completedError is not null) throw completedError;
            return completedInfo!;
        }
    }

    public async Task<RequestInfo> WaitAsync(CancellationToken cancellationToken = default)
    {
        _ = Id;
        using var registration = cancellationToken.Register(() =>
        {
            lock (gate)
                if (id != 0 && !completed) NativeMethods.pmac_request_cancel(id);
        });
        return await Completion.ConfigureAwait(false);
    }

    public void Cancel()
    {
        lock (gate) NativeCheck(NativeMethods.pmac_request_cancel(CheckedId()));
    }

    public byte[] Response() => Copy(false);
    public byte[] Stderr() => Copy(true);

    public unsafe byte[] Match(string pattern, uint group = 0)
    {
        lock (gate)
        {
            var requestId = CheckedId();
            using var pinned = new PinnedText();
            var bytes = pinned.Add(pattern);
            var status = NativeMethods.pmac_request_copy_match(requestId, bytes.Data, bytes.Length, group, IntPtr.Zero, UIntPtr.Zero, out var required);
            if (status != -11 && status != 0) throw Error(status);
            var output = new byte[checked((int)(nuint)required)];
            if (output.Length > 0)
            {
                fixed (byte* pointer = output)
                    status = NativeMethods.pmac_request_copy_match(requestId, bytes.Data, bytes.Length, group, (IntPtr)pointer, (UIntPtr)output.Length, out required);
                NativeCheck(status);
            }
            return output;
        }
    }

    public void Dispose()
    {
        lock (gate)
        {
            if (disposed) return;
            disposed = true;
            if (id != 0) NativeMethods.pmac_request_cancel(id);
            if (completed) Release();
            if (id == 0) GC.SuppressFinalize(this);
        }
    }

    internal void Abandon()
    {
        lock (gate)
        {
            disposed = true;
            if (id != 0) NativeMethods.pmac_request_cancel(id);
            Release();
        }
        GC.SuppressFinalize(this);
    }

    internal int Poll(uint waitMs, out RequestInfo? result)
    {
        lock (gate)
        {
            if (completed) { result = completedInfo; return completedError?.Status ?? 0; }
            try
            {
                var status = NativeMethods.pmac_request_wait(id, waitMs, out var info);
                if (status == -6) { result = null; return status; }
                Finish(info, status);
                result = completedInfo;
                return status;
            }
            catch (Exception error)
            {
                FailPump(error);
                result = null;
                return -26;
            }
        }
    }

    internal void FailPump(Exception error)
    {
        lock (gate)
        {
            if (completed) return;
            if (id != 0) NativeMethods.pmac_request_cancel(id);
            completed = true;
            completedError = new PmacException(-26, $"completion pump failed: {error.Message}", id, ExecutionCertainty.MayHaveExecuted);
            completion.TrySetException(completedError);
            if (disposed) Release();
        }
    }
    private void Finish(NativeMethods.RequestInfo info, int status)
    {
        completedInfo = Convert(info);
        completedError = status == 0 ? null : Error(status);
        completed = true;
        if (completedError is not null) completion.TrySetException(completedError);
        else completion.TrySetResult(completedInfo);
        if (disposed) Release();
    }

    private unsafe byte[] Copy(bool stderr)
    {
        lock (gate)
        {
            var requestId = CheckedId();
            var status = stderr ? NativeMethods.pmac_request_copy_stderr(requestId, IntPtr.Zero, UIntPtr.Zero, out var required)
                                : NativeMethods.pmac_request_copy_response(requestId, IntPtr.Zero, UIntPtr.Zero, out required);
            if (status != -11 && status != 0) throw Error(status);
            var output = new byte[checked((int)(nuint)required)];
            if (output.Length > 0)
            {
                fixed (byte* pointer = output)
                    status = stderr ? NativeMethods.pmac_request_copy_stderr(requestId, (IntPtr)pointer, (UIntPtr)output.Length, out required)
                                    : NativeMethods.pmac_request_copy_response(requestId, (IntPtr)pointer, (UIntPtr)output.Length, out required);
                NativeCheck(status);
            }
            return output;
        }
    }

    private unsafe PmacException Error(int status)
    {
        var errorStatus = NativeMethods.pmac_request_copy_error(id, out var info, IntPtr.Zero, UIntPtr.Zero, out var required);
        if (errorStatus != 0 && errorStatus != -11) return new PmacException(status, $"PMAC request failed with status {status}", id);
        var bytes = new byte[checked((int)(nuint)required)];
        if (bytes.Length > 0)
        {
            fixed (byte* pointer = bytes)
                NativeMethods.pmac_request_copy_error(id, out info, (IntPtr)pointer, (UIntPtr)bytes.Length, out required);
        }
        return new PmacException(status, System.Text.Encoding.UTF8.GetString(bytes), info.RequestId, (ExecutionCertainty)info.Certainty);
    }

    private static RequestInfo Convert(NativeMethods.RequestInfo info) => new(info.RequestId, info.ClientId, (RequestState)info.State, info.Status, (ExecutionCertainty)info.Certainty, info.ConnectionEpoch, info.ElapsedUs, info.ResponseLen, info.StderrLen, info.HasExitCode != 0 ? info.ExitCode : null);
    private void Release()
    {
        if (id == 0) return;
        NativeMethods.pmac_request_release(id);
        id = 0;
    }
    private static void NativeCheck(int status)
    {
        if (status != 0) throw new PmacException(status, $"PMAC request failed with status {status}");
    }
}

internal sealed class PinnedText : IDisposable
{
    private readonly List<GCHandle> handles = new();
    internal NativeMethods.ByteView Add(string? text)
    {
        if (text is null) return default;
        var bytes = System.Text.Encoding.UTF8.GetBytes(text);
        var handle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        handles.Add(handle);
        return new NativeMethods.ByteView { Data = handle.AddrOfPinnedObject(), Length = (UIntPtr)bytes.Length };
    }
    internal NativeMethods.ByteView Add(byte[]? bytes)
    {
        if (bytes is null) return default;
        var handle = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        handles.Add(handle);
        return new NativeMethods.ByteView { Data = handle.AddrOfPinnedObject(), Length = (UIntPtr)bytes.Length };
    }
    public void Dispose() { foreach (var handle in handles) if (handle.IsAllocated) handle.Free(); handles.Clear(); }
}

