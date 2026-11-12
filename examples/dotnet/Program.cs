using Pmac;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

static void Require(bool condition, string message)
{
    if (!condition) throw new Exception(message);
}

static bool ExpectedFailure(int status) => status is -7 or -8 or -13 or -23;

static void ExpectUnconnected(Action operation)
{
    var timer = System.Diagnostics.Stopwatch.StartNew();
    while (true)
    {
        try { operation(); throw new Exception("unconnected request unexpectedly succeeded"); }
        catch (PmacException error) when (error.Status == -3 && timer.ElapsedMilliseconds < 1000)
        {
            Thread.Yield(); // Only an admission rejection is safe to resubmit.
        }
        catch (PmacException error)
        {
            Require(ExpectedFailure(error.Status), $"unexpected native status {error.Status}");
            return;
        }
    }
}
static string Text(NativeMethods.ByteView view) => Marshal.PtrToStringUTF8(view.Data, checked((int)(nuint)view.Length)) ?? "";

static void SeedFailedMember(Client client)
{
    // Key preflight fails before opening a socket; this exercises owned terminal results.
    using var request = client.Connect(new ConnectOptions
    {
        Host = "127.0.0.1", Username = "test", AcceptAnyHostKey = true,
        PrivateKeyPath = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N")),
    });
    try { request.Wait(); throw new Exception("missing private key unexpectedly loaded"); }
    catch (PmacException error) { Require(error.Status == -16, $"unexpected key preflight status {error.Status}"); }
}
// Golden definitions were captured from the old public C++ header, not this implementation.
using (var fixture = System.Text.Json.JsonDocument.Parse(File.ReadAllText(
    Path.Combine(AppContext.BaseDirectory, "legacy-status-fields.json"), System.Text.Encoding.UTF8)))
{
    foreach (var field in fixture.RootElement.GetProperty("fields").EnumerateArray())
    {
        var type = field.GetProperty("status").GetString() == "MotorStatus" ? typeof(MotorStatus) : typeof(CoordStatus);
        var property = type.GetProperty(field.GetProperty("dotnet").GetString()!)!;
        var width = field.GetProperty("width").GetInt32();
        var shift = field.GetProperty("shift").GetInt32();
        var mask = (1UL << width) - 1;
        var values = new List<ulong> { 0, ulong.MaxValue, mask << shift, ulong.MaxValue ^ (mask << shift) };
        for (ulong value = 0; value <= mask; ++value) values.Add(value << shift);
        foreach (var raw in values)
        {
            var actual = Convert.ToUInt64(property.GetValue(Activator.CreateInstance(type, raw)));
            Require(actual == ((raw >> shift) & mask), $"legacy status field {type.Name}.{property.Name} differs");
        }
    }
}
// Check marshalling without contacting a server or keeping credential fixtures.
using (var pinned = new PinnedText())
{
    var token = Convert.ToHexString(RandomNumberGenerator.GetBytes(16));
    var options = new ConnectOptions
    {
        Host = "example.invalid", Username = "test", Password = token,
        PrivateKeyPath = "key-path", PublicKeyPath = "public-path", Passphrase = token,
        KnownHostsPath = "known-hosts", Port = 2222, ControlPoolSize = 2,
    };
    var native = options.Native(pinned);
    Require(Text(native.Password) == token && Text(native.Passphrase) == token, "authentication text was not copied");
    Require(Text(native.PrivateKey) == "key-path" && Text(native.PublicKey) == "public-path", "key options were not copied");
    Require(native.Port == 2222 && native.ControlPoolSize == 2 && native.Flags == 1, "connection options differ");
}

using (var runtime = new Pmac.Runtime())
{
    using var client = runtime.CreateClient(maxRequests: 1);
    Require(client.State().State == ConnectionState.Created, "initial state differs");
    Require(client.PoolState().Ready == 0 && client.PoolMembers().Count == 0, "initial pool differs");
    SeedFailedMember(client);
    Require(client.PoolState().Failed == 1 && client.PoolMembers().Count == 1, "key preflight did not isolate a failed member");
    // Every sync wrapper must release a terminal request before the next submit.
    for (var index = 0; index < 128; ++index)
    {
        ExpectUnconnected(() => client.Command("#1p"));
        ExpectUnconnected(() => client.GetResponse("#1p"));
        ExpectUnconnected(() => client.Shell("pwd"));
        ExpectUnconnected(() => client.Upload("absent", "/tmp/pmac-test"));
        ExpectUnconnected(() => client.Download("/tmp/pmac-test", "absent"));
        ExpectUnconnected(() => client.BufferUpload("absent", "/tmp/pmac-test"));
    }
}

using (var runtime = new Pmac.Runtime())
{
    using var client = runtime.CreateClient(maxRequests: 64);
    SeedFailedMember(client);
    await Task.WhenAll(Enumerable.Range(0, 32).Select(async index =>
    {
        Request request;
        try { request = client.GetResponseAsync($"#{index % 8 + 1}p"); }
        catch (PmacException error) when (error.Status == -23) { return; }
        using (request)
        {
            var requestId = request.Id;
            using var cancellation = new CancellationTokenSource();
            if ((index & 1) == 0) cancellation.Cancel();
            try { await request.WaitAsync(cancellation.Token); throw new Exception("unconnected async request succeeded"); }
            catch (PmacException error)
            {
                Require(ExpectedFailure(error.Status), $"unexpected asynchronous status {error.Status}");
                Require(error.RequestId == requestId, "request diagnostics lost ownership");
                _ = request.Route;
            }
        }
    }));
    // Runtime closes and releases managed clients, and repeated disposal is harmless.
    runtime.Shutdown();
    runtime.Shutdown();
    try { client.State(); throw new Exception("closed client retained an ID"); }
    catch (ObjectDisposedException) { }
}

Require(Marshal.SizeOf<NativeMethods.PoolProfile>() == 24, "pool profile ABI size differs");
Require(Marshal.SizeOf<NativeMethods.PoolState>() == 64, "pool state ABI size differs");
Require(Marshal.SizeOf<NativeMethods.PoolMember>() == 48, "pool member ABI size differs");
Require((new RequestOptions(Infinite: true, ReadPool: true)).Native().Flags == 3, "read-pool request flag differs");

using (var runtime = new Pmac.Runtime(completionCapacity: 1))
{
    var field = typeof(Pmac.Runtime).GetField("completions", System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance)!;
    var pump = (CompletionPump)field.GetValue(runtime)!;
    pump.Reserve();
    var submitted = false;
    try
    {
        runtime.Submit(() => { submitted = true; return 0; });
        throw new Exception("completion admission unexpectedly succeeded");
    }
    catch (PmacException error) { Require(error.Status == -3 && !submitted, "completion pressure reached native submission"); }
    finally { pump.ReleaseSlot(); }
}
static void ManagementFailure(Request request, int expected)
{
    using (request)
    {
        try { request.Wait(); throw new Exception("management operation unexpectedly succeeded"); }
        catch (PmacException error) { Require(error.Status == expected, $"unexpected management status {error.Status}"); }
        _ = request.Route;
    }
}

using (var runtime = new Pmac.Runtime())
{
    using var client = runtime.CreateClient();
    var options = new ConnectOptions
    {
        Host = "127.0.0.1", Username = "test", AcceptAnyHostKey = true,
        PrivateKeyPath = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N")),
    };
    ManagementFailure(client.PreparePoolAsync(options, new PoolProfile(TargetSize: 2, MaxMembers: 4)), -16);
    var state = client.PoolState();
    Require(state.Desired == 2 && state.Ready == 0 && state.Failed == 1, "prepare profile was not retained");
    try { client.ConfirmRecovery(state.Revision + 1, confirmRemoteReleased: true); throw new Exception("stale revision accepted"); }
    catch (PmacException error) { Require(error.Status == -29, "stale revision did not return SUPERSEDED"); }
    // These failed before TCP: no remote session was created by the absent key.
    client.ConfirmRecovery(state.Revision, confirmRemoteReleased: true);
    Require(client.PoolState().ReleaseUncertain == 0, "release acknowledgement did not clear quota");
    ManagementFailure(client.RepairPoolAsync(1), -16);
    ManagementFailure(client.ResizePoolAsync(new PoolProfile(MaxMembers: 4)), -23);
    ManagementFailure(client.DrainPoolAsync(1), -1);
    ManagementFailure(client.RepairMaintenanceAsync(), -23);
}
// Completion notifications are a sequence snapshot, not a consumable work queue.
var rawConfig = new NativeMethods.RuntimeConfig
{
    StructSize = (uint)Marshal.SizeOf<NativeMethods.RuntimeConfig>(), AbiVersion = 2000, WorkerThreads = 1,
};
Require(NativeMethods.pmac_runtime_create(ref rawConfig, out var rawRuntime) == 0, "notification runtime creation failed");
try
{
    ulong cursor = 0;
    var initial = NativeMethods.pmac_runtime_wait_completion(rawRuntime, ref cursor, 0);
    Require(initial is 0 or -6, "initial completion cursor failed");
    var other = cursor;
    Require(NativeMethods.pmac_runtime_wake(rawRuntime) == 0, "manual completion wake failed");
    Require(NativeMethods.pmac_runtime_wait_completion(rawRuntime, ref cursor, 0) == 0, "first completion cursor missed wake");
    Require(NativeMethods.pmac_runtime_wait_completion(rawRuntime, ref other, 0) == 0 && other == cursor, "completion notification was consumed by another cursor");
    Require(NativeMethods.pmac_runtime_wait_completion(rawRuntime, ref cursor, 0) == -6, "unchanged completion cursor did not report WAIT_PENDING");
}
finally
{
    Require(NativeMethods.pmac_runtime_shutdown(rawRuntime, 1000) == 0, "notification runtime shutdown failed");
    Require(NativeMethods.pmac_runtime_release(rawRuntime) == 0, "notification runtime release failed");
}

using (var runtime = new Pmac.Runtime())
{
    var client = runtime.CreateClient();
    var request = client.Connect(new ConnectOptions
    {
        Host = "127.0.0.1", Username = "test", AcceptAnyHostKey = true,
        PrivateKeyPath = Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N")),
    });
    try { request.Wait(); throw new Exception("missing key unexpectedly loaded"); }
    catch (PmacException error) { Require(error.Status == -16, "private-key preflight failed unexpectedly"); }
    try { runtime.Shutdown(); throw new Exception("runtime released while a child request ID remained alive"); }
    catch (PmacException error) { Require(error.Status == -14, "live request did not preserve retryable shutdown"); }
    _ = request.Route; // Results remain locally readable after worker shutdown.
    request.Dispose();
    runtime.Shutdown();
    client.Dispose();
}
// Run unchanged in x64 and WoW64 smoke jobs. Close/Dispose are idempotent;
// a separate State read may legitimately race native release and reject the ID.
using (var runtime = new Pmac.Runtime())
{
    for (var iteration = 0; iteration < 64; ++iteration)
    {
        var client = runtime.CreateClient();
        using var start = new ManualResetEventSlim(false);
        var workers = Enumerable.Range(0, 8).Select(worker => Task.Run(() =>
        {
            start.Wait();
            for (var attempt = 0; attempt < 16; ++attempt)
            {
                if (worker % 3 == 0) client.Close();
                else if (worker % 3 == 1) client.Dispose();
                else
                {
                    try { _ = client.State(); }
                    catch (ObjectDisposedException) { }
                    catch (PmacException error) when (error.Status == -2) { }
                }
            }
        })).ToArray();
        start.Set();
        Task.WaitAll(workers);
        client.Dispose();
    }
    Task.WaitAll(Enumerable.Range(0, 8).Select(_ => Task.Run(runtime.Dispose)).ToArray());
}
[MethodImpl(MethodImplOptions.NoInlining)]
static (WeakReference, ulong) DisposePendingAndDrop(Client client, ushort port)
{
    var request = client.Connect(new ConnectOptions
    {
        Host = "127.0.0.1", Port = port, Username = "test", Password = "test",
        AcceptAnyHostKey = true, TcpProbe = false,
    });
    var id = request.Id;
    request.Dispose();
    return (new WeakReference(request), id);
}

const System.Reflection.BindingFlags Hidden = System.Reflection.BindingFlags.NonPublic | System.Reflection.BindingFlags.Instance;
using (var runtime = new Pmac.Runtime())
{
    var client = runtime.CreateClient();
    var pump = (CompletionPump)typeof(Pmac.Runtime).GetField("completions", Hidden)!.GetValue(runtime)!;
    var blocker = (Request)Activator.CreateInstance(typeof(Request), Hidden, null, new object[] { 0UL, runtime }, null)!;
    var gate = typeof(Request).GetField("gate", Hidden)!.GetValue(blocker)!;
    Monitor.Enter(gate);
    try
    {
        pump.Reserve();
        pump.TrackReserved(blocker);
        Thread.Sleep(100);
        using var listener = new System.Net.Sockets.TcpListener(System.Net.IPAddress.Loopback, 0);
        listener.Start();
        var port = (ushort)((System.Net.IPEndPoint)listener.LocalEndpoint).Port;
        var (weakRequest, id) = DisposePendingAndDrop(client, port);
        GC.Collect();
        GC.WaitForPendingFinalizers();
        GC.Collect();
        Require(!weakRequest.IsAlive, "disposed pending request was retained by the completion pump");
        Require(NativeMethods.pmac_request_cancel(id) == -2, "disposed pending request leaked its native ID after GC");
    }
    finally
    {
        Monitor.Exit(gate);
        blocker.Abandon();
    }
    runtime.Shutdown();
    client.Dispose();
}

[MethodImpl(MethodImplOptions.NoInlining)]
static WeakReference DropRuntime()
{
    var runtime = new Pmac.Runtime();
    return new WeakReference(runtime);
}

var weak = DropRuntime();
GC.Collect();
GC.WaitForPendingFinalizers();
GC.Collect();
Require(!weak.IsAlive, "idle completion pump retained its runtime");
Console.WriteLine("SDK lifecycle smoke passed (768 sync failures, 32 async submissions, authentication, pool ABI/management, completion notifications, retryable release, admission, concurrent Dispose, GC)");
