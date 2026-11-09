# Gameplay loop

What the player does, minute to minute, day to day and across a whole world, and what we build
next to make that loop work. This file is the reference for gameplay decisions: when a new
feature is proposed, it should feed one of the loops below or it waits.

## The pitch in one line

You wake at the edge of the Great Swamp with a satchel and a few torches. The world is beautiful
and indifferent: cold, heat and thin air push back everywhere you go, so every trip out is
planned, and every trip home leaves you better equipped to go further.

## The three loops

### 1. The minute loop: read, gather, adjust

Every few seconds the player is reading the world and the HUD and making a small call.

| Read | Decide | Do |
| --- | --- | --- |
| Temperature meter starts shaking | Push on or warm up? | Light a torch, put on the cloak, get out of the water, head for camp |
| Air meter drops into "Thin air" | How much further up/down? | Slow down (sprinting costs Fatigue too), turn back before "Very thin" |
| Fatigue nearing the mortal line | Rest, eat or risk it? | Eat a meal, sleep at camp, stop sprinting |
| Load gauge goes orange | What's worth carrying? | Drop stone, keep ore, craft a bigger bag |
| Sun getting low | Where do I spend the night? | Walk back, or unroll the bedroll and ring it with torches |
| Creature nearby | Fight, sneak or run? | Crouch to stalk a stag, torch up against spirits, flee a wight |

The meters are the heartbeat of this loop. They must always be visible and always readable at a
glance: position on the bar, colour of the zone, how hard it shakes.

### 2. The day loop: one expedition

One in-game day is 20 real minutes. A good session is one or two expeditions.

1. **Morning at camp.** Check food, clothes and torches. Pick a goal: an ore seam, a barrow, a
   village to trade with, a ridge you saw from the swamp.
2. **Travel out.** The route matters: the swamp is mild but wet (wet = 6 °C colder), the hills are
   dry, the mountains are cold, the deep caves have stale air.
3. **Work the site.** Mine, hunt, loot. The load fills; the meters drift.
4. **The turn.** The core tension: one more block of ore, or go home while the meters are still
   steady? The exposure stages make this a real choice: the first stage only costs stamina, the
   last one can kill.
5. **Return or make camp.** Get back before dark, or set the bedroll down where you are. Night
   brings spirits and a colder temperature.
6. **Spend it.** Craft, cook, put on better clothes, spend a skill point. Tomorrow's trip can go
   further.

### 3. The world loop: push the frontier

Each region pushes on one meter more than the others, so each needs its own preparation. The
frontier moves out as the player's kit and skills catch up.

| Region | What pushes back | What gets you through | What it gives |
| --- | --- | --- | --- |
| Great Swamp (start) | Wet and chilly at night, mist thins the air, bog lurkers | Linen clothes, torches, a camp on dry ground | Reeds, fiber, fish, berries, the first village |
| Hills and forests | Mild; gales in spring and autumn | Better tools, a bigger bag | Wood, flint, stone, boars and stags, barrows |
| Desert and dry lands | Heat: "Hot" by day, "Sweltering" at noon in summer | Light clothes (heavy ones cook you), shade, water, travelling at dawn | Rare stone, ember herbs (planned) |
| Snowy peaks | Cold: "Freezing", and "Deadly cold" on winter nights or in a cold snap | Fur hood and coat, Thick Skin/Hardy skills, the Warmth spell, fire | Ore, fur |
| Deep caves (below y -64) | Stale air: "Thin air" around -150, "Very thin" around -250, "Suffocating" below about -330 | Deep Lungs/Steady Breath skills, short trips, torches to find the way back | The best ore |
| Sky islands (y 340-600) | Thin air and bitter cold together, a long fall | Levitation, Warmth, air skills, furs | First-island XP, rare finds (planned) |

The sky islands are the long-term goal you can see from the very first day: they hang over the
swamp. Getting there takes both an air answer and a cold answer, so it is the natural "mid-game
graduation".

## Survival rules that drive the loop

### Exposure (implemented)

Two meters, always shown, with stages in `assets/data/exposure.ron`:

- **Air** is the oxygen in the air where you stand, as a share of normal. It drops with depth below
  y -64, with height above y 350, in mist (85%) and gales (80%). Under water the meter reads zero and
  you hold your breath for 20 seconds before the lack of air reaches you.
- **Temperature** is what you feel, in °C: biome and season, day and night, weather (overcast -1,
  rain -4, gale -5, storm -7, cold snap -16), being wet (-6), torches and fire nearby (up to +16),
  clothes, the Warmth spell and skills. It follows your surroundings over about half a minute, so a
  quick dip is fine and a long one is not.

Each side has three stages. As a stage gets worse, the meter shakes harder:

| Stage | Cold | Heat | Air | Effect |
| --- | --- | --- | --- | --- |
| 1 | Chilly, below 8 °C | Hot, above 26 °C | Thin air, below 85% | Slight shake. Drains Fatigue slowly, never past the mortal line. No natural healing. |
| 2 | Freezing, below -4 °C | Sweltering, above 34 °C | Very thin, below 60% | Strong shake, faster drain (still stops at the mortal line), a little slower; heat and thin air make you dizzy. |
| 3 | Deadly cold, below -18 °C | Scorching, above 42 °C | Suffocating, below 35% | Violent shake. Real damage on top: it empties Damage, spills into Fatigue and below, and can kill. |

Drain and damage grow the further past a stage's line you are (up to three times). Heat drains
twice as fast while you're carrying a heavy load. A warning shows when you enter a worse stage.

The point of the floor at the mortal line: a cold walk home costs you, but only the far end of a
scale can actually kill you, and you can see that coming on the meter.

### Health, food, sleep (already in)

Four layered bars (Damage, Fatigue, Blood, Injuries), the mortal rule and the healing cap; four food
tiers; sleep at camp. Exposure plugs into these: it never heals, it only wears Fatigue and, at the
far end, Damage.

## What's next, in order

Each step is small enough for one pull request and makes the loop above more real.

1. **Shelter.** Being under a roof and out of the wind cancels weather chill, and an enclosed room
   with a torch warms up. Gives building a survival purpose. (Needs a cheap "roof above / walls
   around" check from the voxel world.)
2. **Campfire block.** A placeable fire that warms a wide area much more than a torch, cooks food
   without a camp, and burns fuel (sticks, logs). Torches stay light, campfires become heat.
3. **Water and thirst (optional, ask Ethan).** A fourth meter that makes the desert harder and
   gives the swamp's water a use. Only if it adds choices rather than chores.
4. **Better clothes ladder.** Light clothes for heat (low insulation, shade), layered furs for cold,
   and a breathing item for deep caves (a reed breathing tube under water, an ember lamp that warns
   of bad air). Each region's answer becomes something you craft.
5. **Expedition goals.** A short list of things to find in each region (an ore, a creature, a
   place), shown in the K map, each giving XP and pointing at the next region. Data in a new
   `assets/data/journey.ron`.
6. **Sky island payoff.** Something only up there worth the trip: a rare ore, a shrine that teaches
   a spell, a view of the whole map.
7. **Tuning pass.** Play a full in-game week from a new world and adjust `exposure.ron`,
   `weather.ron` and `food_tiers.ron` so day 1 is safe, day 3 has a hard night, and the first
   snowy trip needs preparation.

## Questions for Ethan

- Should heat and cold also affect how fast you get hungry (cold burns more food)?
- Should the air meter matter in the swamp itself (bog gas pockets), or stay a depth/height thing?
- Thirst: yes or no?
