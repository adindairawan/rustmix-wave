# Rustmix Wave (Custom Build for EPD397)

This is a custom, upgraded build of the Rustmix Wave firmware for the Waveshare ESP32-S3 3.97-inch e-paper board. 

**For the base firmware, hardware contracts, installation scripts, BLE remote usage, and original documentation, please visit the upstream repository:**  
**[https://github.com/aimindseye/rustmix-wave](https://github.com/aimindseye/rustmix-wave)**

---

## Custom Improvements in this Build

This build introduces several major upgrades focused on battery efficiency, massive memory unlocks for heavy e-reading, and new sensor-driven features.

### Power & Reliability
* **Deep Sleep Optimization:** Implemented deep sleep peripheral teardown and pin isolation to drastically minimize battery drain while preserving the PCF85063 hardware RTC wake-up capabilities.
* **Auto-Reconnect Wi-Fi:** Added a 15-second background Wi-Fi auto-reconnect loop to `src/main.rs`. This recovers from `ESP_ERR_TIMEOUT` boot failures and permanently unblocks weather and clock syncs.
* **Localized Weather:** Hardcoded timezone offsets for Jakarta (WIB) and forced Celsius units to ensure accurate Open-Meteo daily forecasts and temperatures.
* **Config Resilience:** Added a self-healing configuration fallback so a missing or corrupted `WEATHER.TXT` no longer disables the weather card.

### Memory & E-Reader (AO3 Support)
* **8MB PSRAM Unlock:** Enabled the ESP32-S3's 8MB Octal PSRAM via `sdkconfig.defaults` (`CONFIG_SPIRAM_USE_MALLOC=y`). This eliminates the "No space in memory" crash, allowing massive AO3 "Entire Work" `.epub` files to open flawlessly.
* **Smart Wi-Fi Transfer Portal:** Completely rewrote `src/wifi_transfer.rs` to support long filenames. Uploading complex AO3 filenames via LAN automatically shortens titles, converts `.epub` to `.EPU`, and makes them strictly FAT 8.3 compliant on the fly.
* **Collision Avoidance:** The web portal now safely appends iteration suffixes (`~1`, `~2`) to colliding book uploads so existing library files are never accidentally overwritten.

### New Native Apps & Games
* **Shake Dice & Coin Flip:** Built a brand-new native game into `src/games/shake_dice.rs` wired through the Lua event bridge. 
* **Hardware IMU Integration:** Utilizes the QMI8658 IMU so you can physically shake the Waveshare board to roll.
* **Full Dice Set:** Supports a Coin Toss (Heads/Tails), D4, D6, D8, D10, D12, and D20, complete with a rolling history strip and critical hit/miss callouts.

---

## To Improve

- [x] 1. Battery optimization (Implemented deep sleep pin isolation)
- [ ] 2. Online reading (AO3 direct download - deferred 2-4 weeks)
- [x] 3. More games (Added IMU-driven Shake Dice & Coin and more level feature in the default games)
