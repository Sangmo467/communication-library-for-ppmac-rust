using System.Reflection;
using Pmac;
var method = typeof(Client).GetMethod("NativeCommand", BindingFlags.NonPublic | BindingFlags.Static)!;
(uint Timeout, uint Flags) Native(string command, RequestOptions? options = null) {
    var value = method.Invoke(null, new object?[] { command, options })!;
    var type = value.GetType();
    return ((uint)type.GetField("TimeoutMs", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic)!.GetValue(value)!, (uint)type.GetField("Flags", BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic)!.GetValue(value)!);
}
foreach (var command in new[] { "$$$", "$$$***", " \t$$$\r\n" }) {
    if (Native(command).Timeout != 30000) throw new Exception("reset default must be 30 seconds");
    foreach (var timeout in new uint[] { 0, 5000, 45000 })
        if (Native(command, new RequestOptions(TimeoutMs: timeout)).Timeout != timeout) throw new Exception("explicit timeout changed");
    if (Native(command, new RequestOptions()).Timeout != 5000) throw new Exception("explicit options changed");
    if ((Native(command, new RequestOptions(Infinite: true)).Flags & 1) != 1) throw new Exception("infinite option changed");
}
foreach (var command in new[] { "Sys.ServoCount", "$$$ Sys.ServoCount", "$$$\nSys.ServoCount", "$$$**" })
    if (Native(command).Timeout != 5000) throw new Exception("ordinary command timeout changed");
Console.WriteLine("C# reset timeout options passed");