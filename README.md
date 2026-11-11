# Rustmix Wave (Custom Build for EPD397)

This is a custom, upgraded build of the Rustmix Wave firmware for the Waveshare ESP32-S3 3.97-inch e-paper board.

**For the base firmware, hardware contracts, installation scripts, BLE remote usage, and original documentation, please visit the upstream repository:**  
**[https://github.com/aimindseye/rustmix-wave](https://github.com/aimindseye/rustmix-wave)**

---

## Custom Improvements in this Build

This build introduces major upgrades focused on battery efficiency (both active/idle and deep sleep), massive memory unlocks for heavy AO3 e-reading with per-book fanfic resume, and new sensor-driven games.

### Power & Reliability
* **Active & Idle Battery Optimization:**
  * Configured ESP-IDF power management dynamic frequency scaling (`esp_pm_configure` at `40–160 MHz`) in `src/main.rs` alongside `CONFIG_PM_ENABLE` and tickless idle.
  * Gated I2S0 TX/RX clocks on demand in `src/audio/espidf.rs` so the 6.144 MHz MCLK and ESP-IDF APB/light-sleep lock are released whenever audio playback or recording is idle.
  * Added on-demand standby/wake for the QMI8658 IMU (`src/imu.rs`, `src/board_services.rs`) and lowered active ODR from `1000 Hz` to `250 Hz`, keeping the IMU asleep unless a motion screen or motion-controlled game is active.
  * Cached AXP2101 chip-ID verification in `src/power.rs` to cut redundant polling I2C traffic, and enabled `WIFI_PS_MAX_MODEM` when Wi-Fi is connected and idle (`WIFI_PS_MIN_MODEM` during file transfers) in `src/network.rs`.
* **Deep Sleep Optimization (Turned Off):**
  * Powers down external peripherals over I2C before `esp_deep_sleep_start()`: ES8311 DAC/ADC/analog/clock standby (`src/audio/board_codec.rs`), QMI8658 standby (`CTRL7 = 0x00`), and AXP2101 `ALDO3` + battery ADC channel disable (`src/power.rs`).
  * Drops SPI3, I2C0, I2S0, SDMMC1, and Wi-Fi drivers cleanly, resets and isolates all non-wake GPIOs (SDMMC, I2S/amp, SPI/e-paper, I2C, and buttons), and pins flash/PSRAM/GPIO sleep leakage workarounds in `sdkconfig.defaults` while preserving `GPIO0` (BOOT) and RTC wake capabilities.
  * **Silent Charging While Off:** Detects unrequested USB/VBUS boots while in sleep mode (`rustmix_pm/sleeping` NVS flag and AXP2101 `PWRON_STATUS` register `0x20` in `src/power.rs` / `src/main.rs`) and immediately returns to deep sleep before touching the e-paper display, keeping the sleep image on screen while charging. Wake intentionally anytime with the `BOOT` (`GPIO0`) button.
* **Auto-Reconnect Wi-Fi:** Added a 15-second on-demand Wi-Fi reconnect loop in `src/main.rs` when on Network/Weather screens to recover from `ESP_ERR_TIMEOUT` boot failures.
* **Localized Weather & Clock:** Shifts synced UTC timestamps to Jakarta (WIB / UTC+7) and defaults temperature display to Celsius across Home, Environment, and Weather views.
* **Config Resilience:** Added a self-healing configuration fallback so a missing or corrupted `WEATHER.TXT` is repaired automatically on SD mount instead of disabling the weather card.

### Memory & E-Reader (AO3 Fanfic Support)
* **8MB PSRAM Unlock:** Enabled the ESP32-S3's 8MB Octal PSRAM via `sdkconfig.defaults` (`CONFIG_SPIRAM_USE_MALLOC=y`) so large AO3 "Entire Work" `.epub` files open without out-of-memory crashes.
* **AO3 Fanfic Mode & Resume:**
  * Dedicated `/sdcard/RUSTMIX/BOOKS/FANFIC` library tab with AO3 metadata blurb cards (author, rating, warnings, fandoms, relationships, characters, tags, word/chapter counts, and summary) and chapter-at-a-time continuous scroll streaming (`src/epub.rs`, `src/reader.rs`).
  * **Per-Book Location Saving:** Fanfics save and restore their exact chapter and scroll line across sessions (`STATE.TXT`, `POSITS.TXT`, `RECENT.TXT`, `MARKS.TXT`), display `CH X/Y  LINE Z (PCT%)` in the reader, show `Saved: Ch X (PCT%)` badges on library cards (`src/app/screens/reader.rs`), and support chapter-aware TOC navigation.
* **Smart Wi-Fi Transfer Portal:** Rewrote `src/wifi_transfer.rs` to auto-shorten long AO3 filenames, convert `.epub` to `.EPU`, and enforce FAT 8.3 compliance on upload.
* **Collision Avoidance:** Appends iteration suffixes (`~1`, `~2`) to colliding uploads so existing library files are never accidentally overwritten.

### New Native Apps & Games
* **Shake Dice & Coin Flip (`src/games/shake_dice.rs`):** Native game wired through the Lua event bridge using the QMI8658 IMU so you can shake the board to roll. Supports Coin Toss (Heads/Tails), D4, D6, D8, D10, D12, and D20 with a rolling history strip and critical hit/miss callouts.
* **Baba Tilt (`src/games/baba_tilt.rs`):** Native rule-pushing puzzle game (`YOU`, `STOP`, `PUSH`, `WIN`, `SINK`) controlled via board tilt or buttons, complete with undo history and custom level support.
* **Multi-Level Puzzle Progression:** Extended default puzzle games (`Sokoban Tilt`, `Tilt Maze`) with multi-level progression.

---

## To Improve

- [x] 1. Battery optimization (Implemented active/idle DFS + light sleep, I2S/IMU gating, and deep sleep teardown & pin isolation)
- [ ] 2. Online reading (AO3 direct download - deferred 2-4 weeks)
- [x] 3. More games (Added IMU-driven Shake Dice & Coin, Baba Tilt, and multi-level puzzle support)
- [x] 4. Fanfic reading position resume (Implemented per-book chapter & scroll-line saving and library progress badges)
