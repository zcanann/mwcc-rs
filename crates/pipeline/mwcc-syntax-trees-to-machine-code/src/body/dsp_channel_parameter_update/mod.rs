//! Publish a logical channel's parameter bank to its DSP channel.
//!
//! The initialization and steady-state forms share one retained-state schedule.
//! Initialization adds a leading delay call; steady-state publication adds a
//! distance-filter call. Recognition records those phases independently so the
//! common six-lane mixer and filter transaction stays single-purpose.

#[allow(unused_imports)]
use super::*;

mod emit;
mod recognize;

pub(super) struct DirectMemberCall<'a> {
    call: &'a str,
    offset: i16,
}

pub(super) struct DspChannelParameterUpdate<'a> {
    channel_pointer_offset: i16,
    channel_id_offset: i16,
    manager_offset: i16,
    lane_values_offset: i16,
    lane_modes_offset: i16,
    lane_count: u16,
    pitch_offset: i16,
    filter_mode_offset: i16,
    iir_offset: i16,
    fir_offset: i16,
    pause_offset: i16,
    leading: Option<DirectMemberCall<'a>>,
    mixer: &'a str,
    pitch: &'a str,
    iir: &'a str,
    fir: &'a str,
    mode: &'a str,
    distance: Option<DirectMemberCall<'a>>,
    pause: &'a str,
}

impl Generator {
    pub(crate) fn try_dsp_channel_parameter_update(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = recognize::classify(function) else {
            return Ok(false);
        };
        if self.behavior.integer_loop_style
            != mwcc_versions::IntegerLoopStyle::LegacyDependencyFirst
            || self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.behavior.schedule_latency_slots
            || !self.behavior.use_lmw_stmw
        {
            return Ok(false);
        }
        emit::emit(self, &plan);
        Ok(true)
    }
}
