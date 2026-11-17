using System.Reflection;
using System.Runtime.InteropServices;
using Pmac;
var library = NativeLibrary.Load(Environment.GetEnvironmentVariable("PMAC_LIBRARY") ?? throw new Exception("Set PMAC_LIBRARY to request_error_fixture.dll"));
var setCase = Marshal.GetDelegateForFunctionPointer<SetCase>(NativeLibrary.GetExport(library, "test_set_error_case"));
var expected = new[] { "unclosed character class", "error #31", "failed" };
for (var mode = 0; mode < 3; ++mode)
{
    setCase(mode);
    var request = (Request)Activator.CreateInstance(typeof(Request), BindingFlags.Instance | BindingFlags.NonPublic, null, new object?[] { 7UL, null }, null)!;
    try
    {
        request.Match("[");
        throw new Exception("match unexpectedly succeeded");
    }
    catch (PmacException error)
    {
        if (error.Status != (mode == 0 ? -22 : -10) || !error.Message.Contains(expected[mode]) || (mode != 2 && error.Certainty != ExecutionCertainty.Confirmed))
            throw new Exception($"Lost request diagnostic in case {mode}: {error.Status}: {error.Message}");
    }
    finally { request.Dispose(); }
}
Console.WriteLine("C# request diagnostics passed");
[UnmanagedFunctionPointer(CallingConvention.Cdecl)] delegate void SetCase(int value);