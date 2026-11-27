//! Stamina: one bar that every fighting action draws on. Light cuts, heavy
//! cuts, kicks, sprinting and blows caught on your blade all cost some; it
//! refills after a short pause. Run it dry and you're winded for a moment:
//! no cuts, no kicks, no guard, no sprint.
//!
//! The rules (`Stamina`) are pure and tested; the plugin ticks it and draws a
//! thin bar above the hotbar while it isn't full.

use bevy::prelude::*;

use crate::theme::Theme;

pub const MAX: f32 = 100.0;
/// Refilled per second once resting.
const REGEN: f32 = 32.0;
/// Seconds after spending before it refills.
const DELAY: f32 = 0.7;
/// Seconds winded after running dry.
const WINDED: f32 = 1.3;

/// What each action costs.
pub const LIGHT: f32 = 12.0;
pub const HEAVY: f32 = 24.0;
pub const KICK: f32 = 20.0;
/// A cut caught on your blade (a parry costs nothing).
pub const BLOCK_HIT: f32 = 16.0;
/// Per second of sprinting.
pub const SPRINT: f32 = 9.0;

#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Stamina {
    pub value: f32,
    /// Seconds until it starts refilling.
    rest: f32,
    /// Seconds left winded (ran dry).
    pub winded: f32,
}

impl Default for Stamina {
    fn default() -> Self {
        Self { value: MAX, rest: 0.0, winded: 0.0 }
    }
}

impl Stamina {
    /// Can you act at all (not winded, something left)?
    pub fn ready(&self) -> bool {
        self.winded <= 0.0 && self.value > 0.0
    }

    /// Spend `cost` on an action. False (nothing spent) while winded. With
    /// less than the cost left the action still goes, and leaves you winded.
    pub fn spend(&mut self, cost: f32) -> bool {
        if !self.ready() {
            return false;
        }
        self.value -= cost;
        self.rest = DELAY;
        if self.value <= 0.0 {
            self.value = 0.0;
            self.winded = WINDED;
        }
        true
    }

    /// A cut caught on your guard. True if the guard holds; false if it had
    /// too little left and breaks (you're winded and take the blow).
    pub fn block(&mut self) -> bool {
        let holds = self.value >= BLOCK_HIT;
        self.value = (self.value - BLOCK_HIT).max(0.0);
        self.rest = DELAY;
        if !holds {
            self.winded = WINDED;
        }
        holds
    }

    /// Spend a steady drain (sprinting) for `dt` seconds. False when there's
    /// nothing left to spend.
    pub fn drain(&mut self, per_second: f32, dt: f32) -> bool {
        if !self.ready() {
            return false;
        }
        self.value = (self.value - per_second * dt).max(0.0);
        self.rest = self.rest.max(DELAY * 0.5);
        if self.value <= 0.0 {
            self.winded = WINDED;
        }
        true
    }

    pub fn tick(&mut self, dt: f32) {
        self.winded = (self.winded - dt).max(0.0);
        self.rest -= dt;
        if self.rest <= 0.0 {
            self.value = (self.value + REGEN * dt).min(MAX);
        }
    }

    pub fn share(&self) -> f32 {
        self.value / MAX
    }
}

pub struct StaminaPlugin;

impl Plugin for StaminaPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Stamina>()
            .add_systems(crate::menu::GameStartup, spawn_bar)
            .add_systems(crate::menu::GameUpdate, (tick, draw_bar).chain());
    }
}

fn tick(time: Res<Time>, player: Res<crate::player::Player>, mode: Res<crate::player::MoveMode>, mut stamina: ResMut<Stamina>) {
    let dt = time.delta_secs();
    if player.sprinting && *mode == crate::player::MoveMode::Walk {
        stamina.drain(SPRINT, dt);
    }
    stamina.tick(dt);
}

#[derive(Component)]
struct Bar;

#[derive(Component)]
struct Fill;

const BAR_W: f32 = 220.0;

/// A thin bar centred just above the hotbar.
fn spawn_bar(mut commands: Commands, theme: Res<Theme>) {
    commands
        .spawn((
            Bar,
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(8.0 + crate::hud::SLOT + 14.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-BAR_W / 2.0)),
                width: Val::Px(BAR_W),
                height: Val::Px(10.0),
                padding: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            theme.skin("bar"),
            Visibility::Hidden,
        ))
        .with_children(|b| {
            b.spawn((Fill, Node { width: Val::Percent(100.0), height: Val::Percent(100.0), ..default() }, BackgroundColor(Color::srgb(0.9, 0.78, 0.3))));
        });
}

fn draw_bar(stamina: Res<Stamina>, menus: Res<crate::camera::Menus>, mut bar: Query<&mut Visibility, With<Bar>>, mut fill: Query<(&mut Node, &mut BackgroundColor), With<Fill>>) {
    let show = stamina.value < MAX - 0.01 && !menus.still;
    for mut v in &mut bar {
        let want = if show { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    for (mut node, mut color) in &mut fill {
        node.width = Val::Percent(stamina.share() * 100.0);
        // Gold while it refills, red while winded.
        color.0 = if stamina.winded > 0.0 { Color::srgb(0.85, 0.25, 0.2) } else { Color::srgb(0.9, 0.78, 0.3) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_spend_and_it_refills_after_a_pause() {
        let mut s = Stamina::default();
        assert!(s.spend(LIGHT));
        let after = s.value;
        s.tick(DELAY * 0.5);
        assert_eq!(s.value, after, "no refill straight away");
        for _ in 0..200 {
            s.tick(0.05);
        }
        assert_eq!(s.value, MAX);
    }

    #[test]
    fn running_dry_winds_you() {
        let mut s = Stamina::default();
        let mut n = 0;
        while s.spend(HEAVY) {
            n += 1;
        }
        // About four heavy cuts from full.
        assert!((4..=5).contains(&n), "{n}");
        assert!(!s.ready() && s.winded > 0.0);
        assert!(!s.spend(LIGHT), "nothing while winded");
        for _ in 0..40 {
            s.tick(0.05);
        }
        assert!(s.ready(), "back after a breather");
    }

    #[test]
    fn a_guard_breaks_when_too_tired_to_hold() {
        let mut s = Stamina::default();
        assert!(s.block());
        s.value = BLOCK_HIT * 0.5;
        assert!(!s.block(), "too little left: the guard breaks");
        assert!(s.winded > 0.0);
    }

    #[test]
    fn sprinting_drains_steadily_and_stops_when_empty() {
        let mut s = Stamina::default();
        let mut secs = 0.0;
        while s.drain(SPRINT, 0.1) {
            secs += 0.1;
        }
        assert!(secs > 10.0 && secs < 12.5, "{secs}");
    }
}
