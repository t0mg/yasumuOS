# UI_DESIGN.md — Obsidian Handheld Design System (yasumuOS)

## 1. Aesthetic Vision & Philosophy
- **Inspiration**: MinUI, Analogue Pocket / Analogue OS, Teenage Engineering hardware typography.
- **Principles**: Radical minimalism, zero skeuomorphism, zero nested container cards, zero redundant status pills or decorative badges, zero glowing outlines or drop shadows.
- **Atmosphere**: Deep matte dark obsidian bases (`#090B10` / `#0D111A`) with smooth, full-screen radial ambient gradients matching the contextual state accent. Bold sans-serif headlines paired with crisp hardware monospace.

## 2. Hard Rules & Forbidden Anti-Patterns (Never Violate)
- **Variety across the UI, strictly ONE accent color at a time**: different screens and tabs each own a distinct accent color from the palette, but **any single view/screen/tab may only display ONE accent color** across its header indicator/timer, ambient glow, active selection, primary buttons, toggle switches, focus rings, and progress bars — paired strictly with neutral matte surfaces (`#090B10`, `#141824`, `#1E2433`) and neutral text (`#F3F4F6`, `#9CA3AF`). Never mix accent colors within the same screen.
  - **Sole intentional warning exception**: in the Web UI, whenever the daily quota system (`quota_enabled = false`) is turned off, the top-right header displays `"Control Disabled"` in Crimson Rose (`#F43F5E`) on every tab so the parent always sees it. On the handheld launcher, display **nothing** in the top-center header when quotas are off (avoid drawing the child's attention to it).
- **NEVER use decorative status dots, indicator LEDs, pulsing circles, or visual gimmicks** anywhere in the launcher or Web UI (no colored dots next to timers, status labels, or headers). Typography and color carry meaning directly without ornamental glyphs.
- **No generic icons anywhere**: do NOT use generic UI icons, emojis, or decorative pictograms (no clocks, locks, gears, bells, Wi-Fi arcs, etc.) in either the launcher or the Web UI. The **only** supported iconography in the frontend is **hardware button glyphs** representing the physical controls of the device:
  - **Face buttons (`A`, `B`, `X`, `Y`)**: circle with centered mono letter.
  - **Shoulder buttons (`L1`, `L2`, `R1`, `R2`)**: rounded rectangle with centered mono label.
  - **System micro-buttons (`START`, `SELECT`)**: small flat disc / low-profile pill.
  - **Directional inputs**: crisp triangle-shaped arrows (`▲`, `▼`, `◄`, `►`).
  - **Hardware utility buttons**: `MENU`, `VOLUME`, `POWER`, and `RESET` only.
- **Zero outlines / zero borders**: separation comes strictly from spacing and flat matte surface elevation (`#090B10` → `#141824` → `#1E2433`).
- **Zero redundant information**: never repeat metadata (e.g., platform name shown multiple times) or display low-value technical noise (e.g., timezone strings) in permanent chrome.

## 3. Color Palette & Semantics
- **Canvas Matte Base**: `#090B10` / `#0D111A`
- **Surface Matte**: `#141824` (strictly neutral, matte, no borders)
- **Keycap Surface**: `#1E2433`
- **Primary Text**: `#F3F4F6`
- **Muted / Subtitle Text**: `#9CA3AF`
- **State & Tab Accents**:
  - **Signal Orange** (`#FF6600`): Launcher active selection bar, daily quota lock, Web UI **Overview** tab.
  - **Amber Gold** (`#FBBF24`): Clock sync warning, active PIN tumbler, Web UI **Games** tab.
  - **Nocturne Indigo** (`#818CF8`): Bedtime lock, quiet hours, Web UI **Schedule** tab.
  - **Emerald Green** (`#10B981`): Parent portal, Home Wi-Fi connected, healthy battery, Web UI **System** tab.
  - **Crimson Rose** (`#F43F5E`): Incorrect PIN / brute-force cooldown, critical remaining time (≤5m).

## 4. Embedded Typography (`assets/fonts/`)
- **Display / Headlines**: `Sans-ExtraBold.otf` (Cantarell ExtraBold, OFL) — uppercase, bold, tracked, accent-colored.
- **Body / Subtitles / Game List**: `Sans-Regular.otf` (Cantarell Regular, OFL).
- **Hardware Mono**: `Mono-Bold.otf` & `Mono-Regular.otf` (Source Code Pro, OFL) — `YASUMU` mark, remaining time, `88%` battery, hardware button glyphs, Wi-Fi credentials, and URLs.
