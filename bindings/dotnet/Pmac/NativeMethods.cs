using System.Runtime.InteropServices;

namespace Pmac;

internal static class NativeMethods
{
    internal const string Library = "pmac";
    internal const uint AbiVersion = 2000;

    static NativeMethods()
    {
        var libraryPath = Environment.GetEnvironmentVariable("PMAC_LIBRARY");
        if (!string.IsNullOrEmpty(libraryPath)) libraryPath = Path.GetFullPath(libraryPath);
        NativeLibrary.SetDllImportResolver(typeof(NativeMethods).Assembly, (name, assembly, path) =>
        {
            return name == Library && !string.IsNullOrEmpty(libraryPath)
                ? NativeLibrary.Load(libraryPath) : IntPtr.Zero;
        });
    }

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern uint pmac_version_number();
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_get_runtime_info(out RuntimeInfo info);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern uint pmac_abi_version();
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern IntPtr pmac_version_string();
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_runtime_create(ref RuntimeConfig config, out ulong id);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_runtime_wait_completion(ulong runtime, ref ulong cursor, uint waitMs);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_runtime_wake(ulong runtime);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_runtime_shutdown(ulong id, uint waitMs);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_runtime_release(ulong id);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_client_create(ulong runtime, ref ClientConfig config, out ulong id);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_client_close(ulong id, uint mode, uint waitMs);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_client_release(ulong id);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_client_get_state(ulong id, out StateSnapshot state);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_connect_submit(ulong id, ref ConnectOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_command_submit(ulong id, IntPtr command, UIntPtr length, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_get_response_async(ulong id, IntPtr command, UIntPtr length, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_dart_connect_async(ulong id, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_dart_disconnect_async(ulong id, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_dart_get_async(ulong id, uint command, uint address, UIntPtr length, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_dart_set_async(ulong id, uint command, uint address, IntPtr data, UIntPtr length, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_shell_submit(ulong id, IntPtr command, UIntPtr length, uint persistent, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_upload_submit(ulong id, IntPtr local, UIntPtr localLength, IntPtr remote, UIntPtr remoteLength, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_download_submit(ulong id, IntPtr remote, UIntPtr remoteLength, IntPtr local, UIntPtr localLength, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_buffer_upload_submit(ulong id, IntPtr local, UIntPtr localLength, IntPtr remote, UIntPtr remoteLength, uint cleanup, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_file_exists_submit(ulong id, IntPtr remote, UIntPtr length, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_file_remove_submit(ulong id, IntPtr remote, UIntPtr length, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_pool_prepare_async(ulong client, ref ConnectOptions options, ref PoolProfile profile, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_pool_resize_async(ulong client, ref PoolProfile profile, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_pool_repair_async(ulong client, ulong member, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_pool_drain_async(ulong client, ulong member, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_pool_repair_maintenance_async(ulong client, ref RequestOptions options, out ulong request);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_pool_get_state(ulong client, out PoolState state);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_pool_get_members(ulong client, IntPtr members, UIntPtr capacity, out UIntPtr required);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_pool_confirm_recovery(ulong client, ulong expectedRevision, uint flags);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_request_get_route(ulong request, out ulong memberId, out ulong epoch);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_request_wait(ulong id, uint waitMs, out RequestInfo info);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_request_cancel(ulong id);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_request_copy_response(ulong id, IntPtr output, UIntPtr capacity, out UIntPtr required);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_request_copy_stderr(ulong id, IntPtr output, UIntPtr capacity, out UIntPtr required);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_request_copy_error(ulong id, out ErrorInfo info, IntPtr output, UIntPtr capacity, out UIntPtr required);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_request_copy_match(ulong id, IntPtr pattern, UIntPtr length, uint group, IntPtr output, UIntPtr capacity, out UIntPtr required);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_request_release(ulong id);

    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_abort(ulong id, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_coord_enable(ulong id, int n, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_coord_exec_prog(ulong id, int n, int p, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_coord_hold(ulong id, int n, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_coord_run(ulong id, int n, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_plc_enable(ulong id, int n, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_plc_disable(ulong id, int n, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_kill(ulong id, int n, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_enable(ulong id, int n, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_jog_stop(ulong id, int n, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_abs_jog(ulong id, int n, double v, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_inc_jog(ulong id, int n, double v, ref RequestOptions o);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_coord_status(ulong id, int n, ref RequestOptions o, out ulong v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_status(ulong id, int n, ref RequestOptions o, out ulong v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_position(ulong id, int n, ref RequestOptions o, out double v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_desired_position(ulong id, int n, ref RequestOptions o, out double v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_velocity(ulong id, int n, ref RequestOptions o, out double v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_position_error(ulong id, int n, ref RequestOptions o, out double v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_servo_error_count(ulong id, ref RequestOptions o, out uint v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_phase_error_count(ulong id, ref RequestOptions o, out uint v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_servo_count(ulong id, ref RequestOptions o, out uint v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_phase_time(ulong id, ref RequestOptions o, out double v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_servo_time(ulong id, ref RequestOptions o, out double v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_abort_all_flag(ulong id, ref RequestOptions o, out uint v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_plc_running(ulong id, int n, ref RequestOptions o, out uint v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_coord_prog_running(ulong id, int n, ref RequestOptions o, out uint v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_motor_closed_loop(ulong id, int n, ref RequestOptions o, out uint v);
    [DllImport(Library, CallingConvention = CallingConvention.Cdecl)] internal static extern int pmac_coord_motor_enabled(ulong id, int n, ref RequestOptions o, out uint v);

    [StructLayout(LayoutKind.Sequential)] internal struct ByteView { internal IntPtr Data; internal UIntPtr Length; }
    [StructLayout(LayoutKind.Sequential)] internal struct RuntimeConfig { internal uint StructSize, AbiVersion, WorkerThreads, Reserved; }
    [StructLayout(LayoutKind.Sequential)] internal struct ClientConfig { internal uint StructSize, AbiVersion, MaxRequests, MaxFileTasks; internal UIntPtr MaxRetainedBytes; }
    [StructLayout(LayoutKind.Sequential)] internal struct ConnectOptions
    {
        internal uint StructSize, AbiVersion;
        internal ByteView Host, Username, Password, PrivateKey, PublicKey, Passphrase, KnownHosts, PinnedSha256;
        internal ushort Port; internal byte ControlPoolSize, Reserved0; internal uint Flags, TcpProbeTimeoutMs, KeepaliveIntervalMs, KeepaliveMax;
        internal ulong Reserved1, Reserved2, Reserved3, Reserved4;
    }
    [StructLayout(LayoutKind.Sequential)] internal struct RequestOptions { internal uint StructSize, AbiVersion, TimeoutMs, Flags; internal UIntPtr MaxResponseBytes; }
    [StructLayout(LayoutKind.Sequential)] internal struct RequestInfo { internal uint StructSize, AbiVersion; internal ulong RequestId, ClientId; internal uint State; internal int Status; internal uint Certainty, Reserved; internal ulong ConnectionEpoch, ElapsedUs; internal UIntPtr ResponseLen, StderrLen; internal int ExitCode; internal uint HasExitCode; }
    [StructLayout(LayoutKind.Sequential)] internal struct ErrorInfo { internal uint StructSize, AbiVersion; internal int Status; internal uint Certainty; internal ulong RequestId, ConnectionEpoch; }
    [StructLayout(LayoutKind.Sequential)] internal struct StateSnapshot { internal uint StructSize, AbiVersion, State, Closing; internal ulong ConnectionEpoch, LastValidResponseUnixMs; internal UIntPtr ActiveRequests; }
    [StructLayout(LayoutKind.Sequential)] internal struct PoolProfile
    {
        internal uint StructSize, AbiVersion, TargetSize, MaxMembers, MinReady, ConnectTimeoutMs;
    }
    [StructLayout(LayoutKind.Sequential)] internal struct PoolState
    {
        internal uint StructSize, AbiVersion;
        internal ulong Revision;
        internal uint Desired, Ready, Busy, Creating, Failed, Retiring, ReleaseUncertain, MaxMembers;
        internal uint UnknownWrite, MaintenanceActive, MaintenanceFailed;
        internal int LastCreateError;
    }
    [StructLayout(LayoutKind.Sequential)] internal struct PoolMember
    {
        internal uint StructSize, AbiVersion;
        internal ulong MemberId, Epoch;
        internal uint State, Control;
        internal ulong LastValidResponseUnixMs;
        internal int LastError;
        internal uint Reserved;
    }    [StructLayout(LayoutKind.Sequential)] internal struct RuntimeInfo { internal uint StructSize, AbiMajor, AbiMinor, VersionNumber, FeatureFlags, PointerWidth; }
}
