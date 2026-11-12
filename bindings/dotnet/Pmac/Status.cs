namespace Pmac;

public readonly record struct MotorStatus(ulong Raw)
{
    private bool Bit(int shift) => (Raw & (1UL << shift)) != 0;
    private uint Field(int shift) => (uint)((Raw >> shift) & 3);
    public bool TriggerMove => Bit(63); public bool HomeInProgress => Bit(62); public bool MinusLimit => Bit(61); public bool PlusLimit => Bit(60);
    public bool FeWarn => Bit(59); public bool FeFatal => Bit(58); public bool LimitStop => Bit(57); public bool AmpFault => Bit(56);
    public bool SoftMinusLimit => Bit(55); public bool SoftPlusLimit => Bit(54); public bool I2tFault => Bit(53); public bool TriggerNotFound => Bit(52);
    public bool AmpWarn => Bit(51); public bool EncLoss => Bit(50); public bool AuxFault => Bit(49); public bool HomeComplete => Bit(47);
    public bool DesVelZero => Bit(46); public bool ClosedLoop => Bit(45); public bool AmpEna => Bit(44); public bool InPos => Bit(43);
    public bool InterlockStop => Bit(42); public bool BlockRequest => Bit(41); public bool PhaseFound => Bit(40); public bool TriggerSpeedSel => Bit(39);
    public bool GantryHomed => Bit(38); public uint SpindleMotor => Field(36); public bool Csolve => Bit(31); public bool SoftLimit => Bit(30);
    public bool DacLimit => Bit(29); public bool BlDir => Bit(28); public bool SoftLimitDir => Bit(27);
}

public readonly record struct CoordStatus(ulong Raw)
{
    private bool Bit(int shift) => (Raw & (1UL << shift)) != 0;
    private uint Field(int shift) => (uint)((Raw >> shift) & 3);
    public bool AddedDwellDis => Bit(0); public bool SharpCornerStop => Bit(1); public bool Cc3Active => Bit(2); public bool EndDelayActive => Bit(3);
    public uint CcMoveType => Field(4); public bool CcOffReq => Bit(6); public bool CcAddedArc => Bit(7); public bool LookAheadActive => Bit(8);
    public bool LookAheadFlush => Bit(9); public bool LookAheadReCalc => Bit(10); public bool LookAheadChange => Bit(11); public bool LookAheadStop => Bit(12);
    public bool LookAheadDir => Bit(13); public bool LookAheadLookBack => Bit(14); public bool LookAheadWrap => Bit(15); public bool SegStopReq => Bit(16);
    public bool SegEnabled => Bit(17); public bool SegMoveDecel => Bit(18); public bool SegMoveAccel => Bit(19); public uint SegMove => Field(20);
    public uint MoveMode => Field(22); public uint CcMode => Field(24); public bool ContMotion => Bit(26); public bool BlockActive => Bit(27);
    public uint FeedHold => Field(28); public bool SegHaltReq => Bit(30); public bool Csolve => Bit(31); public bool SyncBufferError => Bit(32);
    public bool BufferError => Bit(33); public bool LinToPvtError => Bit(34); public bool PvtError => Bit(35); public bool RunTimeError => Bit(36);
    public bool SoftLimit => Bit(37); public bool Radius2Error => Bit(38); public bool Radius1Error => Bit(39); public bool TimersEnabled => Bit(40);
    public bool BlockRequest => Bit(41); public bool InterlockStop => Bit(42); public bool InPos => Bit(43); public bool AmpEna => Bit(44);
    public bool ClosedLoop => Bit(45); public bool DesVelZero => Bit(46); public bool HomeComplete => Bit(47); public bool TimerEnabled => Bit(48);
    public bool AuxFault => Bit(49); public bool EncLoss => Bit(50); public bool AmpWarn => Bit(51); public bool TriggerNotFound => Bit(52);
    public bool I2tFault => Bit(53); public bool SoftPlusLimit => Bit(54); public bool SoftMinusLimit => Bit(55); public bool AmpFault => Bit(56);
    public bool LimitStop => Bit(57); public bool FeFatal => Bit(58); public bool FeWarn => Bit(59); public bool PlusLimit => Bit(60);
    public bool MinusLimit => Bit(61); public bool HomeInProgress => Bit(62); public bool TriggerMove => Bit(63);
}
