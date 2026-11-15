using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using Pmac;

static class Native {
    [DllImport("pmac", CallingConvention = CallingConvention.Cdecl)]
    internal static extern int pmac_runtime_wake(ulong id);
    [DllImport("pmac", CallingConvention = CallingConvention.Cdecl)]
    internal static extern int pmac_client_release(ulong id);
}
class Program {
    static ulong Id(object value) => (ulong)value.GetType().GetField("id", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(value)!;
    [MethodImpl(MethodImplOptions.NoInlining)]
    static (WeakReference, ulong) AbandonRuntime() { var value = new Pmac.Runtime(); return (new WeakReference(value), Id(value)); }
    [MethodImpl(MethodImplOptions.NoInlining)]
    static (WeakReference, ulong) AbandonClient(Pmac.Runtime runtime) { var value = runtime.CreateClient(); return (new WeakReference(value), Id(value)); }
    static void Collect() { GC.Collect(); GC.WaitForPendingFinalizers(); GC.Collect(); }
    static void Main() {
        var (runtimeWeak, runtimeId) = AbandonRuntime();
        Collect();
        // Do not manually finish native cleanup: the finalizer must do so itself.
        var deadline = DateTime.UtcNow.AddSeconds(5);
        int status;
        do { Thread.Sleep(20); status = Native.pmac_runtime_wake(runtimeId); } while (status == 0 && DateTime.UtcNow < deadline);
        if (runtimeWeak.IsAlive || status != -2) throw new Exception($"Runtime finalizer did not release its ID: {status}");
        using var runtime = new Pmac.Runtime();
        var (clientWeak, clientId) = AbandonClient(runtime);
        Collect();
        // Shutdown must also recover IDs whose managed Client has disappeared.
        runtime.Shutdown(5000);
        if (clientWeak.IsAlive || Native.pmac_client_release(clientId) != -2) throw new Exception("Client finalizer lost its native ID");
        Console.WriteLine("Runtime and Client finalizer regressions passed");
    }
}