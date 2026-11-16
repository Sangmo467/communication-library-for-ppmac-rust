namespace Pmac;

public sealed class Client : IDisposable, IAsyncDisposable
{
    private readonly Runtime runtime;
    private ulong id;
    private readonly object lifecycle = new();
    internal Client(ulong id, Runtime runtime) { this.id = id; this.runtime = runtime; }

    public Request Connect(ConnectOptions options)
    {
        using var pinned = new PinnedText();
        var native = options.Native(pinned);
        return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_connect_submit(Id, ref native, out var request)); return request; });
    }

    public async Task ConnectAsync(ConnectOptions options, CancellationToken cancellationToken = default) { using var request = Connect(options); await request.WaitAsync(cancellationToken).ConfigureAwait(false); }
    public Request CommandAsync(string command, RequestOptions? options = null) { using var pinned = new PinnedText(); var bytes = pinned.Add(command); var native = NativeCommand(command, options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_command_submit(Id, bytes.Data, bytes.Length, ref native, out var request)); return request; }); }
    public byte[] Command(string command, RequestOptions? options = null) { using var request = CommandAsync(command, options); request.Wait(); return request.Response(); }
    public Request GetResponseAsync(string command, RequestOptions? options = null) { using var pinned = new PinnedText(); var bytes = pinned.Add(command); var native = NativeCommand(command, options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_get_response_async(Id, bytes.Data, bytes.Length, ref native, out var request)); return request; }); }
    public byte[] GetResponse(string command, string? pattern = null, RequestOptions? options = null) { using var request = GetResponseAsync(command, options); request.Wait(); return string.IsNullOrEmpty(pattern) ? request.Response() : request.Match(pattern); }
    public Request DartConnectAsync(RequestOptions? options = null) { var native = Native(options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_dart_connect_async(Id, ref native, out var request)); return request; }); }
    public void DartConnect(RequestOptions? options = null) { using var request = DartConnectAsync(options); request.Wait(); }
    public Request DartDisconnectAsync(RequestOptions? options = null) { var native = Native(options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_dart_disconnect_async(Id, ref native, out var request)); return request; }); }
    public void DartDisconnect(RequestOptions? options = null) { using var request = DartDisconnectAsync(options); request.Wait(); }
    public Request DartGetAsync(uint command, uint address, nuint length, RequestOptions? options = null) { var native = Native(options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_dart_get_async(Id, command, address, (UIntPtr)length, ref native, out var request)); return request; }); }
    public byte[] DartGet(uint command, uint address, nuint length, RequestOptions? options = null) { using var request = DartGetAsync(command, address, length, options); request.Wait(); return request.Response(); }
    public Request DartSetAsync(uint command, uint address, byte[] data, RequestOptions? options = null) { ArgumentNullException.ThrowIfNull(data); using var pinned = new PinnedText(); var bytes = pinned.Add(data); var native = Native(options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_dart_set_async(Id, command, address, bytes.Data, bytes.Length, ref native, out var request)); return request; }); }
    public void DartSet(uint command, uint address, byte[] data, RequestOptions? options = null) { using var request = DartSetAsync(command, address, data, options); request.Wait(); }
    public Request ShellAsync(string command, bool persistent = true, RequestOptions? options = null) { using var pinned = new PinnedText(); var bytes = pinned.Add(command); var native = Native(options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_shell_submit(Id, bytes.Data, bytes.Length, persistent ? 1u : 0u, ref native, out var request)); return request; }); }
    public byte[] Shell(string command, bool persistent = true, RequestOptions? options = null) { using var request = ShellAsync(command, persistent, options); request.Wait(); return request.Response(); }

    public Request UploadAsync(string local, string remote, RequestOptions? options = null) { using var pinned = new PinnedText(); var l = pinned.Add(local); var r = pinned.Add(remote); var native = Native(options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_upload_submit(Id, l.Data, l.Length, r.Data, r.Length, ref native, out var request)); return request; }); }
    public Request DownloadAsync(string remote, string local, RequestOptions? options = null) { using var pinned = new PinnedText(); var r = pinned.Add(remote); var l = pinned.Add(local); var native = Native(options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_download_submit(Id, r.Data, r.Length, l.Data, l.Length, ref native, out var request)); return request; }); }
    public Request BufferUploadAsync(string local, string remote, bool cleanup = false, RequestOptions? options = null) { using var pinned = new PinnedText(); var l = pinned.Add(local); var r = pinned.Add(remote); var native = Native(options); return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_buffer_upload_submit(Id, l.Data, l.Length, r.Data, r.Length, cleanup ? 1u : 0u, ref native, out var request)); return request; }); }
    public void Upload(string local, string remote, RequestOptions? options = null) { using var request = UploadAsync(local, remote, options); request.Wait(); }
    public void Download(string remote, string local, RequestOptions? options = null) { using var request = DownloadAsync(remote, local, options); request.Wait(); }
    public void BufferUpload(string local, string remote, bool cleanup = false, RequestOptions? options = null) { using var request = BufferUploadAsync(local, remote, cleanup, options); request.Wait(); }
    public bool FileExists(string remote, RequestOptions? options = null) { using var pinned = new PinnedText(); var r = pinned.Add(remote); var native = Native(options); using var request = runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_file_exists_submit(Id, r.Data, r.Length, ref native, out var requestId)); return requestId; }); request.Wait(); return request.Response().AsSpan().SequenceEqual("1"u8); }
    public void RemoveFile(string remote, RequestOptions? options = null) { using var pinned = new PinnedText(); var r = pinned.Add(remote); var native = Native(options); using var request = runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_file_remove_submit(Id, r.Data, r.Length, ref native, out var requestId)); return requestId; }); request.Wait(); }

    public StateSnapshot State() { Runtime.Check(NativeMethods.pmac_client_get_state(Id, out var state)); return new StateSnapshot((ConnectionState)state.State, state.Closing != 0, state.ConnectionEpoch, state.LastValidResponseUnixMs, state.ActiveRequests); }
    public bool IsConnected => State().State == ConnectionState.Ready;
    public void Close(uint waitMs = 1000)
    {
        lock (lifecycle)
        {
            if (id != 0) Runtime.Check(NativeMethods.pmac_client_close(id, 0, waitMs));
        }
    }

    public Request PreparePoolAsync(ConnectOptions options, PoolProfile profile)
    {
        using var pinned = new PinnedText();
        var connection = options.Native(pinned);
        var native = profile.Native();
        return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_pool_prepare_async(Id, ref connection, ref native, out var request)); return request; });
    }

    public Request ResizePoolAsync(PoolProfile profile)
    {
        var native = profile.Native();
        return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_pool_resize_async(Id, ref native, out var request)); return request; });
    }

    public Request RepairPoolAsync(ulong memberId = 0, RequestOptions? options = null)
    {
        var native = Native(options);
        return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_pool_repair_async(Id, memberId, ref native, out var request)); return request; });
    }

    public Request DrainPoolAsync(ulong memberId = 0, RequestOptions? options = null)
    {
        var native = Native(options);
        return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_pool_drain_async(Id, memberId, ref native, out var request)); return request; });
    }

    public Request RepairMaintenanceAsync(RequestOptions? options = null)
    {
        var native = Native(options);
        return runtime.Submit(() => { Runtime.Check(NativeMethods.pmac_pool_repair_maintenance_async(Id, ref native, out var request)); return request; });
    }

    public PoolState PoolState()
    {
        Runtime.Check(NativeMethods.pmac_pool_get_state(Id, out var state));
        return new PoolState(state.Revision, state.Desired, state.Ready, state.Busy, state.Creating,
            state.Failed, state.Retiring, state.ReleaseUncertain, state.MaxMembers, state.UnknownWrite != 0,
            state.MaintenanceActive != 0, state.MaintenanceFailed != 0, state.LastCreateError);
    }

    public unsafe IReadOnlyList<PoolMember> PoolMembers()
    {
        var status = NativeMethods.pmac_pool_get_members(Id, IntPtr.Zero, UIntPtr.Zero, out var required);
        for (var attempt = 0; attempt < 4; ++attempt)
        {
            if (status != 0 && status != -11) Runtime.Check(status);
            var members = new NativeMethods.PoolMember[checked((int)(nuint)required)];
            fixed (NativeMethods.PoolMember* pointer = members)
                status = NativeMethods.pmac_pool_get_members(Id, (IntPtr)pointer, (UIntPtr)members.Length, out required);
            if (status == -11) continue;
            Runtime.Check(status);
            return members.Take(checked((int)(nuint)required)).Select(member => new PoolMember(
                member.MemberId, member.Epoch, (PoolMemberState)member.State, member.Control != 0,
                member.LastValidResponseUnixMs, member.LastError)).ToArray();
        }
        throw new PmacException(-12, "pool membership changed repeatedly while copying the snapshot");
    }

    public void ConfirmRecovery(ulong expectedRevision, bool acknowledgeUnknownWrites = false, bool confirmRemoteReleased = false)
    {
        var flags = (acknowledgeUnknownWrites ? 1u : 0u) | (confirmRemoteReleased ? 2u : 0u);
        Runtime.Check(NativeMethods.pmac_pool_confirm_recovery(Id, expectedRevision, flags));
    }
    public void Abort(RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_abort(Id, ref n)); }
    public void CoordEnable(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_coord_enable(Id, v, ref n)); }
    public void CoordExecProg(int v, int p, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_coord_exec_prog(Id, v, p, ref n)); }
    public void CoordHold(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_coord_hold(Id, v, ref n)); }
    public void CoordRun(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_coord_run(Id, v, ref n)); }
    public void PlcEnable(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_plc_enable(Id, v, ref n)); }
    public void PlcDisable(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_plc_disable(Id, v, ref n)); }
    public void MotorKill(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_kill(Id, v, ref n)); }
    public void MotorEnable(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_enable(Id, v, ref n)); }
    public void MotorJogStop(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_jog_stop(Id, v, ref n)); }
    public void MotorAbsJog(int v, double value, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_abs_jog(Id, v, value, ref n)); }
    public void MotorIncJog(int v, double value, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_inc_jog(Id, v, value, ref n)); }
    public ulong CoordStatus(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_coord_status(Id, v, ref n, out var value)); return value; }
    public ulong MotorStatus(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_status(Id, v, ref n, out var value)); return value; }
    public double MotorPosition(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_position(Id, v, ref n, out var value)); return value; }
    public double MotorDesiredPosition(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_desired_position(Id, v, ref n, out var value)); return value; }
    public double MotorVelocity(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_velocity(Id, v, ref n, out var value)); return value; }
    public double MotorPositionError(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_position_error(Id, v, ref n, out var value)); return value; }
    public uint ServoErrorCount(RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_servo_error_count(Id, ref n, out var value)); return value; }
    public uint PhaseErrorCount(RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_phase_error_count(Id, ref n, out var value)); return value; }
    public uint ServoCount(RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_servo_count(Id, ref n, out var value)); return value; }
    public double PhaseTime(RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_phase_time(Id, ref n, out var value)); return value; }
    public double ServoTime(RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_servo_time(Id, ref n, out var value)); return value; }
    public bool AbortAll(RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_abort_all_flag(Id, ref n, out var value)); return value != 0; }
    public bool PlcRunning(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_plc_running(Id, v, ref n, out var value)); return value != 0; }
    public bool CoordProgRunning(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_coord_prog_running(Id, v, ref n, out var value)); return value != 0; }
    public bool MotorClosedLoop(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_motor_closed_loop(Id, v, ref n, out var value)); return value != 0; }
    public bool CoordMotorEnabled(int v, RequestOptions? o = null) { var n = Native(o); Runtime.Check(NativeMethods.pmac_coord_motor_enabled(Id, v, ref n, out var value)); return value != 0; }

    private static NativeMethods.RequestOptions NativeCommand(string command, RequestOptions? options) =>
        (options ?? new RequestOptions(TimeoutMs: command?.Trim() is "$$$" or "$$$***" ? 30000u : 5000u)).Native();
    private static NativeMethods.RequestOptions Native(RequestOptions? options) => (options ?? new RequestOptions()).Native();
    private ulong Id { get { var value = Volatile.Read(ref id); return value == 0 ? throw new ObjectDisposedException(nameof(Client)) : value; } }
    ~Client() { try { var value = Volatile.Read(ref id); if (value != 0) { runtime.FinalizeClient(value); } } catch { } }
    internal void ReleaseAfterRuntimeShutdown()
    {
        lock (lifecycle)
        {
            if (id == 0) return;
            Runtime.Check(NativeMethods.pmac_client_release(id));
            runtime.ForgetClient(id);
            Volatile.Write(ref id, 0UL);
            GC.SuppressFinalize(this);
        }
    }
    public void Dispose()
    {
        lock (lifecycle)
        {
            if (id == 0) return;
            Close(); // Retain the ID when shutdown remains pending.
            ReleaseAfterRuntimeShutdown();
        }
    }
    public async ValueTask DisposeAsync() { await Task.Run(Dispose).ConfigureAwait(false); }
}
