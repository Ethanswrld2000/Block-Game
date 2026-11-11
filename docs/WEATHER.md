# Weather and clouds

Every weather kind and what it does to the sky. The numbers come from
`assets/data/weather.ron` (the weather) and `assets/data/seasons.ron` (how
often each comes up), plus the three cloud layers in `crates/game/src/clouds.rs`.
The same table is in `docs/weather.csv` for a spreadsheet. If you change the
RON files, update both.

| weather | name in files | lasts (game hours) | lasts (real minutes) | cloud cover | cloud shade | low deck (y 200) | middle puffs (y 236) | high wisps (y 300) | visibility (blocks) | sunlight | rain/snow | wind (blocks/s) | lightning | felt chill (°C) | air | spring % | summer % | autumn % | winter % |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Clear | clear | 6-14 | 5.0-11.7 | 0.1 | 1.0 | 0% | 3% | 5% | 1400 | 1.0 | 0.0 | 3 | no | 0 | 1.0 | 27% | 54% | 18% | 19% |
| Overcast | overcast | 4-10 | 3.3-8.3 | 0.75 | 0.8 | 50% | 87% | 74% | 900 | 0.55 | 0.0 | 5 | no | -1.0 | 1.0 | 20% | 15% | 24% | 25% |
| Rain | rain | 3-8 | 2.5-6.7 | 0.9 | 0.6 | 88% | 100% | 89% | 260 | 0.35 | 0.7 | 6 | no | -4.0 | 1.0 | 27% | 8% | 18% | 25% |
| Thunderstorm | storm | 3-6 | 2.5-5.0 | 1 | 0.4 | 100% | 100% | 100% | 170 | 0.2 | 1.0 | 12 | yes | -7.0 | 0.9 | 7% | 15% | 6% | - |
| Mist | mist | 3-7 | 2.5-5.8 | 0.5 | 0.9 | 0% | 55% | 47% | 70 | 0.7 | 0.0 | 1 | no | -2.0 | 0.85 | 13% | 8% | 18% | 12% |
| Cold Snap | cold_snap | 8-18 | 6.7-15.0 | 0.06 | 1.0 | 0% | 0% | 1% | 1400 | 1.0 | 0.0 | 4 | no | -16.0 | 1.0 | - | - | 6% | 19% |
| Gale | gale | 3-6 | 2.5-5.0 | 0.85 | 0.6 | 75% | 100% | 84% | 400 | 0.45 | 0.2 | 24 | no | -5.0 | 0.8 | 7% | - | 12% | - |

How to read it:

- **Lasts**: random between the two numbers. One in-game hour is 50 real
  seconds (a day is 20 minutes). Holding T fast-forwards it with the clock.
- **Cloud cover** is the weather's dial, 0 to 1. Each layer turns that into
  its own cover between its `from` and `to` (low 0.55-0.95, middle
  0.08-0.85, high 0.05-1); the layer columns show where each one sits, 0% = gone,
  100% = as thick as it gets. **Cloud shade** is how white the clouds are
  (1 = white, lower = grey).
- **Wind** is its strength. Rain and snow drift with it and grass and leaves
  sway with it; the clouds drift with its square root (about 1.7 blocks/s on a
  clear day, 5 in a gale). Its direction turns slowly on its own and veers a
  little with each new weather (`crates/game/src/wind.rs`), and the clouds
  follow it round.
- **Visibility** is how far you see before fog, before biome haze. **Sunlight**
  multiplies the sun. **Rain/snow** falls as snow below freezing. **Air** below
  1 makes it harder to breathe. Under about 500 blocks of visibility the fog
  starts to cover the sky too (`sky::fog_veil`); in mist the sun, moon and
  stars are gone.
- **Rain/snow**: snow falls when it's below freezing where you are, and keeps
  falling until it's a little above (so walking near the snow line doesn't
  flick it back and forth).
- **Season %** is the chance of that weather when the season picks one. The
  weather never picks itself again, so the real share of the next pick is a
  little higher for the others.

## How a change happens

1. When a weather starts, the game rolls how long it lasts and picks the next
   one right away (by the season at the moment it ends). The panel in the top
   right shows the weather, the next one and the time left, with a bar.
2. The clouds don't change while a weather lasts.
3. A minute before the change (`weather::CLOUD_LEAD_SECONDS`) the clouds
   start turning into the next weather's cover and shade; the panel line turns
   gold ("Rain rolling in 0:48").
4. At the change, rain, fog, light, wind, chill and air blend in over a
   minute (`BLEND_SECONDS`). Lightning starts halfway through.

F4 skips to the following weather at once (clouds and all blend over a minute).
`BLOCKGAME_WEATHER=rain` holds one weather for screenshots; the panel then
says "holding".
