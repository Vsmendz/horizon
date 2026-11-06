use glam::{Mat4, Vec3};
use horizon_core::geometry::trig::Mth;

pub fn item_anchor(invert: f32, inverse_height: f32) -> Mat4 {
    Mat4::from_translation(Vec3::new(
        invert * 0.56,
        -0.52 - inverse_height * 0.6,
        -0.72,
    ))
}

pub fn apply_swing(mut m: Mat4, invert: f32, attack: f32) -> Mat4 {
    let pi = std::f32::consts::PI;
    let root = attack.sqrt();
    m *= Mat4::from_translation(Vec3::new(
        invert * -0.4 * (root * pi).msin(),
        0.2 * (root * std::f32::consts::TAU).msin(),
        -0.2 * (attack * pi).msin(),
    ));
    m *= Mat4::from_rotation_y(
        (invert * (45.0 - (attack * attack * pi).msin() * 20.0)).to_radians(),
    );
    m *= Mat4::from_rotation_z((invert * (root * pi).msin() * -20.0).to_radians());
    m *= Mat4::from_rotation_x(((root * pi).msin() * -80.0).to_radians());
    m *= Mat4::from_rotation_y((invert * -45.0f32).to_radians());
    m
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hand {
    Main,
    Off,
}
impl Hand {
    pub const BOTH: [Hand; 2] = [Hand::Main, Hand::Off];
}

/// `SwingAnimation`: its type (`SwingAnimationType` ordinal) and duration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwingAnimation {
    pub kind: u8,
    pub duration: i32,
}
impl SwingAnimation {
    /// `SwingAnimation.DEFAULT`: WHACK for 6 ticks.
    pub const DEFAULT: SwingAnimation = SwingAnimation {
        kind: 1,
        duration: 6,
    };
}

/// `LivingEntity.SwingDescription`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwingDescription {
    pub hand: Hand,
    pub animation: SwingAnimation,
    pub duration_ticks: i32,
}

/// `LivingEntity.SwingState`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SwingState {
    pub current: Option<SwingDescription>,
    pub ticks: i32,
    pub old_animation: f32,
    pub animation: f32,
}
impl SwingState {
    /// `startIfAble`: a swing restarts unless the current one is in its
    /// first half (past its first tick).
    pub fn start_if_able(&mut self, hand: Hand, animation: SwingAnimation, duration: i32) -> bool {
        if let Some(c) = self.current
            && self.ticks <= c.duration_ticks / 2
            && self.ticks > 0
        {
            return false;
        }
        let swing = SwingDescription {
            hand,
            animation,
            duration_ticks: duration,
        };
        if self.current != Some(swing) {
            self.old_animation = 0.0;
            self.animation = 0.0;
        }
        self.current = Some(swing);
        self.ticks = 0;
        true
    }
    /// `tick` (from `LivingEntity.baseTick`).
    pub fn tick(&mut self) {
        self.old_animation = self.animation;
        if let Some(c) = self.current {
            if c.duration_ticks > 0 {
                self.animation = (self.ticks as f32 / c.duration_ticks as f32).min(1.0);
            }
            let ticks = self.ticks;
            self.ticks += 1;
            if ticks > c.duration_ticks {
                self.current = None;
                self.animation = 0.0;
            }
        } else {
            self.animation = 0.0;
        }
    }
    /// `getAnimation(partialTicks)`: a restarted swing keeps moving forward.
    pub fn animation(&self, partial: f32) -> f32 {
        if self.animation >= self.old_animation {
            crate::camera::lerp(partial, self.old_animation, self.animation)
        } else {
            crate::camera::lerp(partial, self.old_animation, self.animation + 1.0)
        }
    }
    pub fn swinging(&self) -> bool {
        self.current.is_some()
    }
    /// The hand of the current swing (`state.attackHand`), main by default.
    pub fn attack_hand(&self) -> Hand {
        self.current.map_or(Hand::Main, |c| c.hand)
    }
}

/// `LivingEntity.getModifiedSwingDuration`: `dig_speed` is
/// `MobEffectUtil.getDigSpeedAmplification` while haste or conduit power is
/// active, `fatigue` the mining fatigue amplifier.
pub fn swing_duration(
    animation: SwingAnimation,
    dig_speed: Option<i32>,
    fatigue: Option<i32>,
) -> i32 {
    let duration = animation.duration;
    if let Some(amplification) = dig_speed {
        duration - (1 + amplification)
    } else if let Some(amplifier) = fatigue {
        duration + (1 + amplifier) * 2
    } else {
        duration
    }
}

/// `FirstPersonHandsAndItems`' equip heights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HandHeights {
    pub main: f32,
    pub old_main: f32,
    pub off: f32,
    pub old_off: f32,
}
impl Default for HandHeights {
    fn default() -> Self {
        HandHeights {
            main: 0.0,
            old_main: 0.0,
            off: 0.0,
            old_off: 0.0,
        }
    }
}
impl HandHeights {
    /// `FirstPersonHandsAndItems.tick`'s heights. `changed` says per hand
    /// whether the visible item differs from the held one once
    /// `shouldInstantlyReplaceVisibleItem` has run; `swap_scale` is
    /// `getItemSwapScale(1.0F)`. Returns whether each hand's visible item
    /// is replaced at the end.
    pub fn tick(&mut self, hands_busy: bool, swap_scale: f32, changed: [bool; 2]) -> [bool; 2] {
        self.old_main = self.main;
        self.old_off = self.off;
        if hands_busy {
            self.main = (self.main - 0.4).clamp(0.0, 1.0);
            self.off = (self.off - 0.4).clamp(0.0, 1.0);
        } else {
            let attack = swap_scale;
            let main_target = if changed[0] {
                0.0
            } else {
                attack * attack * attack
            };
            let off_target = if changed[1] { 0.0 } else { 1.0 };
            self.main += (main_target - self.main).clamp(-0.4, 0.4);
            self.off += (off_target - self.off).clamp(-0.4, 0.4);
        }
        [self.main < 0.1, self.off < 0.1]
    }
    /// `itemUsed`: the hand drops to the bottom and rises again.
    pub fn item_used(&mut self, hand: Hand) {
        match hand {
            Hand::Main => self.main = 0.0,
            Hand::Off => self.off = 0.0,
        }
    }
    /// `Mth.lerp(partialTicks, oMainHandHeight, mainHandHeight)` and the
    /// off hand's.
    pub fn interpolated(&self, partial: f32) -> [f32; 2] {
        [
            crate::camera::lerp(partial, self.old_main, self.main),
            crate::camera::lerp(partial, self.old_off, self.off),
        ]
    }
}
