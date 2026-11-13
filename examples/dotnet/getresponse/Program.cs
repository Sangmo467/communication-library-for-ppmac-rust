using System.Text;
using Pmac;
using PmacRuntime = Pmac.Runtime;

namespace PmacGetResponseExample;

internal static class Program
{
    private static int Main(string[] args)
    {
        Console.OutputEncoding = new UTF8Encoding(false);
        if (args.Length == 1 && args[0] is "--help" or "-h")
        {
            Console.WriteLine("Usage: Pmac.GetResponse [command [pattern]] | --check");
            Console.WriteLine("Default command: Sys.ServoTime; no extra test commands are sent.");
            Console.WriteLine("A supplied command is executed on the controller and may have side effects.");
            Console.WriteLine("Environment: PMAC_LIBRARY, PMAC_HOST, PMAC_KNOWN_HOSTS, PMAC_PASSWORD.");
            return 0;
        }
        if (args.Length > 2)
        {
            Console.Error.WriteLine("Use --help for command and pattern arguments.");
            return 2;
        }

        PmacRuntime? runtime = null;
        Client? client = null;
        var exitCode = 0;
        try
        {
            var library = RequiredEnvironment("PMAC_LIBRARY");
            if (!File.Exists(library)) throw new FileNotFoundException("PMAC_LIBRARY does not name an existing native library.", library);
            if (args.Length == 1 && args[0] == "--check")
            {
                // Load the library and inspect ABI information without opening a connection.
                var info = PmacRuntime.NativeInfo;
                Console.WriteLine($"pmac {PmacRuntime.NativeVersion}; ABI {info.AbiMajor}.{info.AbiMinor}; {info.PointerWidth}-bit");
                if (info.AbiMajor != 2 || info.PointerWidth != (uint)(IntPtr.Size * 8))
                    throw new InvalidOperationException("Native ABI major or process bitness is incompatible.");
            }
            else
            {
                var connection = new ConnectOptions
                {
                    Host = RequiredEnvironment("PMAC_HOST"),
                    Username = "root",
                    KnownHostsPath = RequiredEnvironment("PMAC_KNOWN_HOSTS"),
                    Password = Environment.GetEnvironmentVariable("PMAC_PASSWORD"),
                };
                var command = args.Length > 0 ? args[0] : "Sys.ServoTime";
                var pattern = args.Length > 1 ? args[1] : null;
                if (string.IsNullOrWhiteSpace(command)) throw new ArgumentException("command must not be empty.");
                if (args.Length > 0)
                    Console.Error.WriteLine("Executing the supplied command once; it may change controller state. No automatic retry.");

                runtime = new PmacRuntime();
                client = runtime.CreateClient();
                using (var request = client.Connect(connection))
                {
                    request.Wait(); // synchronous: connection and command stay on this calling thread
                }
                var response = client.GetResponse(command, pattern);
                var text = Encoding.UTF8.GetString(response);
                Console.Write(text);
                if (!text.EndsWith('\n')) Console.WriteLine();
            }
        }
        catch (PmacException error)
        {
            Report(error); // Preserve error# text and execution certainty from the native request.
            exitCode = 1;
        }
        catch (Exception error)
        {
            Report(error);
            exitCode = 1;
        }
        finally
        {
            // Report cleanup failures separately, preserving any earlier device error text.
            exitCode |= Dispose(client);
            exitCode |= Dispose(runtime);
        }
        return exitCode;
    }

    private static string RequiredEnvironment(string name)
    {
        var value = Environment.GetEnvironmentVariable(name);
        return !string.IsNullOrWhiteSpace(value) ? value : throw new ArgumentException($"Set {name} before running this example.");
    }

    private static int Dispose(IDisposable? value)
    {
        if (value is null) return 0;
        try { value.Dispose(); return 0; }
        catch (Exception error)
        {
            Console.Error.WriteLine($"Cleanup failed for {value.GetType().Name}:");
            Report(error);
            return 1;
        }
    }
    private static void Report(Exception error)
    {
        Console.Error.WriteLine(error.Message);
        if (error is PmacException pmac)
            Console.Error.WriteLine($"Status={pmac.Status}; Certainty={pmac.Certainty}; RequestId={pmac.RequestId}");
    }
}
