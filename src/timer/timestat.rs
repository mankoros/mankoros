use super::TimeVal;

pub struct TimeStat {
    /// user time in us
    utime_us: usize,
    /// system time in us
    stime_us: usize,
    /// set when entering user mode
    user_tick: usize,
    /// set when entering kernel mode
    kernel_tick: usize,
    /// start time
    start_tick: usize,
}

impl TimeStat {
    pub fn new() -> Self {
        let start_tick = super::get_time_us();
        Self {
            utime_us: 0,
            stime_us: 0,
            user_tick: 0,
            kernel_tick: 0,
            start_tick,
        }
    }

    pub fn clear(&mut self) {
        self.utime_us = 0;
        self.stime_us = 0;
        self.user_tick = 0;
        self.kernel_tick = 0;
        self.start_tick = super::get_time_us();
    }

    /// switch from kernel to user
    /// update user tick
    /// update kernel time
    pub fn kernel_to_user(&mut self) {
        let now = super::get_time_us();
        let delta = now - self.kernel_tick;
        self.stime_us += delta;
        self.kernel_tick = now;
    }

    /// switch from user to kernel
    /// update kernel tick
    /// update user time
    pub fn user_to_kernel(&mut self) {
        let now = super::get_time_us();
        let delta = now - self.user_tick;
        self.utime_us += delta;
        self.user_tick = now;
    }

    /// when switch into this lporc
    pub fn switch_into(&mut self) {
        self.kernel_tick = super::get_time_us();
    }

    /// when switch out this proc
    pub fn switch_out(&mut self) {
        let delta = super::get_time_us() - self.kernel_tick;
        self.stime_us += delta;
    }

    /// output utime and stime in TimeVal format
    pub fn output(&self, utime: &mut TimeVal, stime: &mut TimeVal) {
        *utime = self.utime_us.into();
        *stime = self.stime_us.into();
    }

    /// output utime and stime in us
    pub fn output_us(&self) -> (usize, usize) {
        (self.utime_us, self.stime_us)
    }
}
