#ifndef PMAC_STATUS_HPP
#define PMAC_STATUS_HPP

#include <stdint.h>

namespace pmac {
namespace detail {
inline bool bit(uint64_t value, unsigned shift) { return (value & (uint64_t(1) << shift)) != 0; }
inline uint8_t field(uint64_t value, unsigned shift) { return uint8_t((value >> shift) & 3); }
}

struct MotorStatus {
    uint64_t raw;
    explicit MotorStatus(uint64_t value = 0) : raw(value) {}
    bool triggerMove() const { return detail::bit(raw, 63); }
    bool homeInProgress() const { return detail::bit(raw, 62); }
    bool minusLimit() const { return detail::bit(raw, 61); }
    bool plusLimit() const { return detail::bit(raw, 60); }
    bool feWarn() const { return detail::bit(raw, 59); }
    bool feFatal() const { return detail::bit(raw, 58); }
    bool limitStop() const { return detail::bit(raw, 57); }
    bool ampFault() const { return detail::bit(raw, 56); }
    bool softMinusLimit() const { return detail::bit(raw, 55); }
    bool softPlusLimit() const { return detail::bit(raw, 54); }
    bool i2tFault() const { return detail::bit(raw, 53); }
    bool triggerNotFound() const { return detail::bit(raw, 52); }
    bool ampWarn() const { return detail::bit(raw, 51); }
    bool encLoss() const { return detail::bit(raw, 50); }
    bool auxFault() const { return detail::bit(raw, 49); }
    bool homeComplete() const { return detail::bit(raw, 47); }
    bool desVelZero() const { return detail::bit(raw, 46); }
    bool closedLoop() const { return detail::bit(raw, 45); }
    bool ampEna() const { return detail::bit(raw, 44); }
    bool inPos() const { return detail::bit(raw, 43); }
    bool interlockStop() const { return detail::bit(raw, 42); }
    bool blockRequest() const { return detail::bit(raw, 41); }
    bool phaseFound() const { return detail::bit(raw, 40); }
    bool triggerSpeedSel() const { return detail::bit(raw, 39); }
    bool gantryHomed() const { return detail::bit(raw, 38); }
    uint8_t spindleMotor() const { return detail::field(raw, 36); }
    bool csolve() const { return detail::bit(raw, 31); }
    bool softLimit() const { return detail::bit(raw, 30); }
    bool dacLimit() const { return detail::bit(raw, 29); }
    bool blDir() const { return detail::bit(raw, 28); }
    bool softLimitDir() const { return detail::bit(raw, 27); }
};

struct CoordStatus {
    uint64_t raw;
    explicit CoordStatus(uint64_t value = 0) : raw(value) {}
    bool addedDwellDis() const { return detail::bit(raw, 0); }
    bool sharpCornerStop() const { return detail::bit(raw, 1); }
    bool cc3Active() const { return detail::bit(raw, 2); }
    bool endDelayActive() const { return detail::bit(raw, 3); }
    uint8_t ccMoveType() const { return detail::field(raw, 4); }
    bool ccOffReq() const { return detail::bit(raw, 6); }
    bool ccAddedArc() const { return detail::bit(raw, 7); }
    bool lookAheadActive() const { return detail::bit(raw, 8); }
    bool lookAheadFlush() const { return detail::bit(raw, 9); }
    bool lookAheadReCalc() const { return detail::bit(raw, 10); }
    bool lookAheadChange() const { return detail::bit(raw, 11); }
    bool lookAheadStop() const { return detail::bit(raw, 12); }
    bool lookAheadDir() const { return detail::bit(raw, 13); }
    bool lookAheadLookBack() const { return detail::bit(raw, 14); }
    bool lookAheadWrap() const { return detail::bit(raw, 15); }
    bool segStopReq() const { return detail::bit(raw, 16); }
    bool segEnabled() const { return detail::bit(raw, 17); }
    bool segMoveDecel() const { return detail::bit(raw, 18); }
    bool segMoveAccel() const { return detail::bit(raw, 19); }
    uint8_t segMove() const { return detail::field(raw, 20); }
    uint8_t moveMode() const { return detail::field(raw, 22); }
    uint8_t ccMode() const { return detail::field(raw, 24); }
    bool contMotion() const { return detail::bit(raw, 26); }
    bool blockActive() const { return detail::bit(raw, 27); }
    uint8_t feedHold() const { return detail::field(raw, 28); }
    bool segHaltReq() const { return detail::bit(raw, 30); }
    bool csolve() const { return detail::bit(raw, 31); }
    bool syncBufferError() const { return detail::bit(raw, 32); }
    bool bufferError() const { return detail::bit(raw, 33); }
    bool linToPvtError() const { return detail::bit(raw, 34); }
    bool pvtError() const { return detail::bit(raw, 35); }
    bool runTimeError() const { return detail::bit(raw, 36); }
    bool softLimit() const { return detail::bit(raw, 37); }
    bool radius2Error() const { return detail::bit(raw, 38); }
    bool radius1Error() const { return detail::bit(raw, 39); }
    bool timersEnabled() const { return detail::bit(raw, 40); }
    bool blockRequest() const { return detail::bit(raw, 41); }
    bool interlockStop() const { return detail::bit(raw, 42); }
    bool inPos() const { return detail::bit(raw, 43); }
    bool ampEna() const { return detail::bit(raw, 44); }
    bool closedLoop() const { return detail::bit(raw, 45); }
    bool desVelZero() const { return detail::bit(raw, 46); }
    bool homeComplete() const { return detail::bit(raw, 47); }
    bool timerEnabled() const { return detail::bit(raw, 48); }
    bool auxFault() const { return detail::bit(raw, 49); }
    bool encLoss() const { return detail::bit(raw, 50); }
    bool ampWarn() const { return detail::bit(raw, 51); }
    bool triggerNotFound() const { return detail::bit(raw, 52); }
    bool i2tFault() const { return detail::bit(raw, 53); }
    bool softPlusLimit() const { return detail::bit(raw, 54); }
    bool softMinusLimit() const { return detail::bit(raw, 55); }
    bool ampFault() const { return detail::bit(raw, 56); }
    bool limitStop() const { return detail::bit(raw, 57); }
    bool feFatal() const { return detail::bit(raw, 58); }
    bool feWarn() const { return detail::bit(raw, 59); }
    bool plusLimit() const { return detail::bit(raw, 60); }
    bool minusLimit() const { return detail::bit(raw, 61); }
    bool homeInProgress() const { return detail::bit(raw, 62); }
    bool triggerMove() const { return detail::bit(raw, 63); }
};
}
#endif
