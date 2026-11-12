class _Status:
    _bits = {}
    _fields = {}
    def __init__(self, raw): self.raw = int(raw)
    def __getattr__(self, name):
        if name in self._bits: return bool(self.raw & (1 << self._bits[name]))
        if name in self._fields: return (self.raw >> self._fields[name]) & 3
        raise AttributeError(name)

class MotorStatus(_Status):
    _bits = dict(trigger_move=63, home_in_progress=62, minus_limit=61, plus_limit=60, fe_warn=59, fe_fatal=58, limit_stop=57, amp_fault=56, soft_minus_limit=55, soft_plus_limit=54, i2t_fault=53, trigger_not_found=52, amp_warn=51, enc_loss=50, aux_fault=49, home_complete=47, des_vel_zero=46, closed_loop=45, amp_ena=44, in_pos=43, interlock_stop=42, block_request=41, phase_found=40, trigger_speed_sel=39, gantry_homed=38, csolve=31, soft_limit=30, dac_limit=29, bl_dir=28, soft_limit_dir=27)
    _fields = dict(spindle_motor=36)

class CoordStatus(_Status):
    _bits = dict(added_dwell_dis=0, sharp_corner_stop=1, cc3_active=2, end_delay_active=3, cc_off_req=6, cc_added_arc=7, look_ahead_active=8, look_ahead_flush=9, look_ahead_recalc=10, look_ahead_change=11, look_ahead_stop=12, look_ahead_dir=13, look_ahead_look_back=14, look_ahead_wrap=15, seg_stop_req=16, seg_enabled=17, seg_move_decel=18, seg_move_accel=19, cont_motion=26, block_active=27, seg_halt_req=30, csolve=31, sync_buffer_error=32, buffer_error=33, lin_to_pvt_error=34, pvt_error=35, run_time_error=36, soft_limit=37, radius2_error=38, radius1_error=39, timers_enabled=40, block_request=41, interlock_stop=42, in_pos=43, amp_ena=44, closed_loop=45, des_vel_zero=46, home_complete=47, timer_enabled=48, aux_fault=49, enc_loss=50, amp_warn=51, trigger_not_found=52, i2t_fault=53, soft_plus_limit=54, soft_minus_limit=55, amp_fault=56, limit_stop=57, fe_fatal=58, fe_warn=59, plus_limit=60, minus_limit=61, home_in_progress=62, trigger_move=63)
    _fields = dict(cc_move_type=4, seg_move=20, move_mode=22, cc_mode=24, feed_hold=28)
