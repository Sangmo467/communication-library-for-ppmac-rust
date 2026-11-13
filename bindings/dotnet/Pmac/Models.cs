namespace Pmac;

public enum RequestState : uint { Queued, Sending, AwaitingResponse, Completing, Completed }
public enum ExecutionCertainty : uint { NotSent, MayHaveExecuted, Confirmed }
public enum ConnectionState : uint { Created, Connecting, Authenticating, Initializing, Ready, Suspect, Recovering, Disconnected, Closing, Closed }

public sealed class PmacException : Exception
{
    public PmacException(int status, string message, ulong requestId = 0, ExecutionCertainty certainty = ExecutionCertainty.NotSent)
        : base(message) { Status = status; RequestId = requestId; Certainty = certainty; }
    public int Status { get; }
    public ulong RequestId { get; }
    public ExecutionCertainty Certainty { get; }
}

public sealed record RequestOptions(uint TimeoutMs = 5000, bool Infinite = false, nuint MaxResponseBytes = 1024 * 1024, bool ReadPool = false, bool Passthrough = false)
{
    internal NativeMethods.RequestOptions Native() => new()
    {
        StructSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf<NativeMethods.RequestOptions>(),
        AbiVersion = NativeMethods.AbiVersion,
        TimeoutMs = TimeoutMs,
        Flags = (Infinite ? 1u : 0u) | (ReadPool ? 2u : 0u) | (Passthrough ? 4u : 0u),
        MaxResponseBytes = MaxResponseBytes,
    };
}

public sealed record RequestInfo(
    ulong RequestId, ulong ClientId, RequestState State, int Status,
    ExecutionCertainty Certainty, ulong ConnectionEpoch, ulong ElapsedMicroseconds,
    nuint ResponseLength, nuint StderrLength, int? ExitCode);

public sealed record StateSnapshot(ConnectionState State, bool Closing, ulong ConnectionEpoch, ulong LastValidResponseUnixMs, nuint ActiveRequests);

public sealed class ConnectOptions
{
    public required string Host { get; init; }
    public required string Username { get; init; }
    public string? Password { get; init; }
    public ushort Port { get; init; } = 22;
    public string? PrivateKeyPath { get; init; }
    public string? PublicKeyPath { get; init; }
    public string? Passphrase { get; init; }
    public string? KnownHostsPath { get; init; }
    public byte[]? PinnedSha256 { get; init; }
    public bool AcceptAnyHostKey { get; init; }
    public bool TcpProbe { get; init; } = true;
    public uint TcpProbeTimeoutMs { get; init; } = 600;
    public uint KeepaliveIntervalMs { get; init; } = 1000;
    public uint KeepaliveMax { get; init; } = 3;
    public byte ControlPoolSize { get; init; } = 1;

    internal NativeMethods.ConnectOptions Native(PinnedText pinned) => new()
    {
        StructSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf<NativeMethods.ConnectOptions>(),
        AbiVersion = NativeMethods.AbiVersion,
        Host = pinned.Add(Host), Username = pinned.Add(Username), Password = pinned.Add(Password),
        PrivateKey = pinned.Add(PrivateKeyPath), PublicKey = pinned.Add(PublicKeyPath), Passphrase = pinned.Add(Passphrase),
        KnownHosts = pinned.Add(KnownHostsPath), PinnedSha256 = pinned.Add(PinnedSha256), Port = Port,
        ControlPoolSize = ControlPoolSize, Flags = (TcpProbe ? 1u : 0u) | (AcceptAnyHostKey ? 2u : 0u),
        TcpProbeTimeoutMs = TcpProbeTimeoutMs, KeepaliveIntervalMs = KeepaliveIntervalMs, KeepaliveMax = KeepaliveMax,
    };
}

public sealed record PoolProfile(uint TargetSize = 1, uint MaxMembers = 1, uint MinReady = 1, uint ConnectTimeoutMs = 60000)
{
    internal NativeMethods.PoolProfile Native() => new()
    {
        StructSize = (uint)System.Runtime.InteropServices.Marshal.SizeOf<NativeMethods.PoolProfile>(),
        AbiVersion = NativeMethods.AbiVersion, TargetSize = TargetSize, MaxMembers = MaxMembers,
        MinReady = MinReady, ConnectTimeoutMs = ConnectTimeoutMs,
    };
}

public sealed record PoolState(
    ulong Revision, uint Desired, uint Ready, uint Busy, uint Creating, uint Failed,
    uint Retiring, uint ReleaseUncertain, uint MaxMembers, bool UnknownWrite,
    bool MaintenanceActive, bool MaintenanceFailed, int LastCreateError);

public sealed record PoolMember(
    ulong MemberId, ulong Epoch, PoolMemberState State, bool Control,
    ulong LastValidResponseUnixMs, int LastError);

public enum PoolMemberState : uint { Ready = 0, Busy = 1, Failed = 2, Retiring = 3, ReleaseUncertain = 4, Creating = 5, Drained = 6 }
public sealed record RequestRoute(ulong MemberId, ulong Epoch);
public sealed record RuntimeInfo(uint AbiMajor, uint AbiMinor, uint VersionNumber, uint FeatureFlags, uint PointerWidth);
