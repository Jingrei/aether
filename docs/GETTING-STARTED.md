# Aether Sound: setup and user guide

This guide takes you from a fresh download to spatial sound on your headphones or speakers, then explains every
control. Parts 1–3 take about 10 minutes.

- [Part 1: Install what you need](#part-1-install-what-you-need)
- [Part 2: Build Aether](#part-2-build-aether)
- [Part 3: Route your PC's audio through Aether](#part-3-route-your-pcs-audio-through-aether)
- [Part 4: Using Aether](#part-4-using-aether)
- [Part 5: Optional extras](#part-5-optional-extras)
- [Troubleshooting](#troubleshooting)
- [Turning Aether off or uninstalling](#turning-aether-off-or-uninstalling)

---

## How it works (30-second version)

Aether is not a driver, so it can't hook into Windows audio directly. Instead:

```
Apps (Spotify, games, browser…)
        │  play into
        ▼
"CABLE Input"  ← a virtual sound card, set as the Windows default
        │
        ▼
     Aether     ← captures the cable, applies spatial sound + enhancements
        │
        ▼
Your real headphones / speakers
```

This means Aether must be running for you to hear anything while the cable is your default device. Keep that in mind
if your sound ever "disappears": see [Troubleshooting](#troubleshooting).

---

## Part 1: Install what you need

### Step 1. Install a virtual audio cable

1. Download **VB-Audio Virtual Cable** (free) from <https://vb-audio.com/Cable/>.
2. Extract the zip, right-click `VBCABLE_Setup_x64.exe`, and choose **Run as administrator** → **Install Driver**.
3. Restart your PC when asked.

> Already installed on this PC: you can skip this step. To check, open **Settings → System → Sound**. If you see
> *CABLE Input* under Output, it's installed.

### Step 2. Install Rust (only needed to build from source)

Skip this if someone gave you a ready-built `aether.exe`.

1. Download `rustup-init.exe` from <https://rustup.rs>.
2. Open PowerShell in the download folder and run:

   ```powershell
   .\rustup-init.exe -y --default-host x86_64-pc-windows-gnu
   ```

   This installs for your user only (no admin needed). It uses the GNU toolchain, so you don't need Visual Studio.
3. Close and reopen PowerShell, then check it works:

   ```powershell
   cargo --version
   ```

   If PowerShell says `cargo` isn't recognized, use the full path `$env:USERPROFILE\.cargo\bin\cargo.exe`, or
   add that folder to your PATH.

---

## Part 2: Build Aether

### Step 3. Build the app

In PowerShell, from the project folder (`DTS Clone`):

```powershell
cargo build --release
```

The first build downloads libraries and takes about 4–5 minutes. The result is a single file:

```
target\release\aether.exe
```

Optional: run the tests to confirm everything works on your machine.

```powershell
cargo test --workspace
```

### Step 4 (recommended). Put the app somewhere permanent

If you plan to use **Start with Windows**, copy `aether.exe` to a stable folder first, for example
`C:\Users\<you>\AppData\Local\Aether\aether.exe`. The startup entry remembers the exact path; if you move the file
later, turn *Start with Windows* off and on again.

---

## Part 3: Route your PC's audio through Aether

### Step 5. Make the virtual cable your default output

1. Open **Settings → System → Sound**.
2. Under **Output**, choose **CABLE Input (VB-Audio Virtual Cable)**.

You'll hear nothing at this point. That's expected: audio now goes into the cable and Aether isn't running yet.

> Tip: set both **CABLE Input** and your real headphones/speakers to the same sample rate (48,000 Hz is best). In the
> classic Sound control panel (`mmsys.cpl`), open the device's **Properties → Advanced → Default Format**. If the rates
> differ, Aether still works, but it shows a yellow warning and has to resample.

### Step 6. Start Aether

Double-click `aether.exe`. Two things appear:

- **The control panel window** (opens automatically).
- **A teal tray icon** near the clock. If you don't see it, click the **^** arrow to show hidden icons. You can drag it
  onto the taskbar to keep it visible.

### Step 7. Check the routing

At the top right of the control panel, the status line should read something like:

```
● CABLE Output (VB-Audio Virtual Cable) → Speakers (…) · 48.0 kHz · ~38 ms
```

- **Left of the arrow** is what Aether is processing. It should be **CABLE Output**.
- **Right of the arrow** is where you'll hear it. It should be your real headphones or speakers.

If the device on the right is wrong:

1. Open the **Settings** tab.
2. Under **Play on**, choose your real headphones or speakers.

Automatic mode picks the first real (non-virtual) device it finds, which isn't always the one you want.

### Step 8. Play something

Play music or a video. You should hear it with spatial processing, and the **Output** spectrum and **IN/OUT** meters
should move. Flip the **On/Off** switch (top right) to compare processed and original sound instantly.

---

## Part 4: Using Aether

### The layout

| Area | What it's for |
|---|---|
| Top bar | Output mode (Headphones / Speakers / Stereo), status, and the **On/Off** switch |
| Left: **Profiles** | Pick a sound profile, or create and import your own |
| Center: **Spatial sound editor** | See the virtual room and speakers, and play a test sound you can move around |
| Center: **Output** | Live spectrum, input/output meters, CPU load |
| Right: **Sound / EQ / Headphones / Settings** | Fine-tune the active profile and app settings |

### Step 9. Choose an output mode

| Mode | Use it for | What it does |
|---|---|---|
| **Headphones** | Any headphones or earbuds | Places the sound on virtual speakers around you, outside your head |
| **Speakers** | Laptop or desktop speakers in front of you | Same spatial rendering plus crosstalk cancellation for a wider stage. Headphone tuning is skipped in this mode |
| **Stereo** | When you want the original image | No virtualization. Bass, dialog, EQ, tuning and leveler still apply |

Sit centered between your speakers in Speakers mode; the effect depends on your position.

### Step 10. Pick a profile

Click a profile on the left. The change is instant.

| Profile | Best for |
|---|---|
| **Natural** | Everyday listening. A small room with minimal coloration |
| **Spacious** | A bigger, more open, enveloping sound |
| **Gaming · Neutral** | Competitive games. Dry and precise |
| **Gaming · Balanced** | Most games. Accuracy plus some room, and a footstep lift |
| **Gaming · Spacious / Spacious 2** | Immersive single-player games. A wide arena-like stage |
| **Movies · Balanced** | TV and films. Clear dialog, solid bass |
| **Movies · Spacious** | Cinema feel. Tall height layer, long decay |
| **Music** | Keeps the stereo mix intact in a pleasant room |
| **Voice** | Podcasts, calls, YouTube talk. Dry, centered, leveled |
| **Night** | Late-night movies. Evens out loud scenes and lifts dialog |

### Step 11. Fine-tune with the Sound tab

Every slider changes the **currently selected profile** and saves automatically.

**Stage**

| Slider | Effect |
|---|---|
| Upmix | How much stereo content is spread to center, surround and height speakers |
| Center focus | How strongly centered sound (usually dialog) is pulled to the center speaker |
| Front width | Angle of the front left/right speakers. Higher is wider |
| Surround angle | Position of the side speakers |
| Height | Level of the overhead speakers |
| Distance | How far away the virtual speakers are |

**Room**

| Slider | Effect |
|---|---|
| Room size | From a small booth to a large hall |
| Reflections | Strength of the early wall echoes. These give the sense of space and help sound feel outside your head |
| Reverb | Level of the late reverberant tail |
| Decay | How long the reverb lasts |
| Damping | How "soft" the walls are. Higher sounds warmer and darker |

**Enhance**

| Slider | Effect |
|---|---|
| Bass boost | Deeper bass. In Speakers mode it also adds harmonics, so small speakers *sound* deeper |
| Dialog clarity | Lifts and clears speech |
| Detail | Brighter high end. Helps with footsteps and positional cues |
| Volume leveler | Evens out quiet and loud passages |
| Preamp | Overall level of this profile |

The Stage and Room sliders are greyed out in Stereo mode because they don't apply there.

### Step 12. Save your own profiles

| To… | Do this |
|---|---|
| Create a profile | Select a profile to start from, click **＋** (top of the Profiles list) or **Duplicate**, and enter a name. It appears under **Custom** |
| Rename a profile | Click its name at the top of the Sound tab, type, and press Enter |
| Undo changes to a built-in profile | Built-in profiles you've edited show a small blue dot. Select one and click **Reset** |
| Delete a custom profile | Select it and click **Delete** |
| Share a profile | Click **Export**. You get a `.aether.json` file |
| Load a shared profile | Click **⇪** (top of the Profiles list) and choose a `.aether.json` file |

### Step 13. Shape the tone with the EQ tab

Ten bands from 31 Hz to 16 kHz, each ±12 dB. The curve at the top shows the combined result. **Flat** resets all
bands. The EQ is part of the profile, so each profile keeps its own EQ.

### Step 14. Correct your headphones (Headphones tab)

Every headphone model has its own frequency response. Correcting it makes the spatial effect more convincing.

- **Quick option:** pick the generic curve that matches your headphones: *Over-Ear*, *On-Ear*, *In-Ear* or *Earbuds*.
  *Bypass* turns correction off.
- **Best option: import a measured correction for your exact model.**
  1. Go to <https://github.com/jaakkopasanen/AutoEq> and search the `results` folder for your headphone model.
  2. Open that model's folder and find the file ending in **`ParametricEQ.txt`**.
  3. In Aether, open **Headphones → Import an AutoEQ / Equalizer APO preset**.
  4. Either click **Open file…** and choose the downloaded file, or paste its contents into the box.
  5. Enter a name, then click **Import**. It's selected automatically.

Remove an imported tuning with the **×** next to it. Headphone tuning applies in Headphones and Stereo modes, not in
Speakers mode.

### Step 15. Try the spatial sound editor

1. Tick **Play test sound**. You'll hear bursts of soft noise.
2. Drag the glowing ball around the head in the room view. The sound moves with it.
3. Raise the **Height** slider (right side) to move it overhead.
4. Untick **Play test sound** when you're done. It is never saved, so it's always off when Aether starts.

This is the quickest way to check that the effect works and to compare profiles.

### Step 16. Choose a quality level (Settings tab)

| Quality | Use when | Difference |
|---|---|---|
| **Best sound** | Plugged in (default) | Full room reflections, height speakers, longest filters |
| **Balanced** | Most laptops on battery | Fewer reflections |
| **Battery** | Saving power | Direct sound only, height folded down, no reverb |

The **DSP %** readout (above the meters) shows how much of the available processing time is used. It's typically 2–5%
at Best.

### Step 17. Use the tray icon

| Action | How |
|---|---|
| Open the control panel | Left-click the tray icon |
| Quick controls | Right-click the icon: **Enabled**, **Output**, **Profile**, **Quality**, **Start with Windows**, **Quit** |

The icon turns grey when processing is off. Closing the control panel window does **not** stop Aether; it keeps
running in the tray. Use **Quit** to exit fully.

### Step 18. Start with Windows (optional)

Tick **Start with Windows** in the Settings tab or the tray menu. Aether then starts silently in the tray at login,
without opening the window. Do Step 4 first so the path doesn't change.

---

## Part 5: Optional extras

### Real surround from games and movies (7.1)

By default Windows sends stereo to the cable, and Aether upmixes it. For games and films with real surround, give
Aether the actual 7.1 channels instead:

1. Press **Win + R**, type `mmsys.cpl`, and press Enter.
2. On the **Playback** tab, select **CABLE Input**, then click **Configure**.
3. Choose **7.1 Surround** and click Next through to **Finish**.
4. On the **Recording** tab, open **CABLE Output → Properties → Advanced**. If a multichannel format is offered,
   choose the 8-channel one at the same rate as your output.
5. In Aether, click **Settings → Refresh devices**. The Source list should now show `CABLE Output … (8 ch)`.

Each channel is now rendered at its own virtual speaker position, with no guessing needed.

### Choosing devices manually

In **Settings**:

- **Source**: what Aether processes. *Automatic* uses CABLE Output if it exists. Entries starting with
  **Loopback:** capture whatever another output device is playing.
- **Play on**: where you hear the result. *Automatic* uses the Windows default, or the first real device if the
  default is a virtual cable.

Aether refuses to capture and play on the same device, because that would create a feedback loop.

### Command-line tools

Run these from PowerShell in the folder that contains `aether.exe`:

```powershell
.\aether.exe devices                       # list devices Aether can see
.\aether.exe profiles                      # list profile and tuning ids
.\aether.exe render in.wav out.wav --profile movies_spacious --mode headphones --quality best
.\aether.exe render in.wav out.wav --bypass   # unprocessed copy, for A/B comparison
.\aether.exe --background                  # start in the tray without opening the window
```

`render` accepts any WAV from mono up to 7.1.4 and writes a stereo 32-bit float WAV. It's useful for comparing
profiles on the same clip.

---

## Troubleshooting

| Problem | Fix |
|---|---|
| **No sound at all** | Aether probably isn't running while *CABLE Input* is your Windows default. Start Aether, or switch the Windows output back to your real device. |
| **Sound comes from the wrong device** | Settings → **Play on** → pick the right device. |
| **Status shows an error in red** | Read the message in the yellow banner. Usually a device was unplugged or renamed. Click **Refresh devices** or choose the device again. Aether also retries automatically every few seconds. |
| **Yellow warning about sample rates** | Set CABLE Input, CABLE Output and your real device to the same rate, ideally 48,000 Hz (Step 5 tip). |
| **"No system-audio source found"** | The virtual cable isn't installed or wasn't detected. Do Step 1, then click **Refresh devices**. |
| **Crackles or dropouts** | Lower **Quality** to Balanced, close heavy background apps, and make sure the sample rates match. |
| **Sound is slightly behind the video** | Expected delay is about 40 ms, which most people don't notice. Bluetooth headphones add their own delay on top. Use wired headphones for competitive gaming. |
| **Volume keys feel different** | With the cable as default, the Windows volume slider controls the level *going into* Aether. Use Settings → **Output gain** if everything is too quiet or loud overall. |
| **Effect sounds too echoey or too dry** | Lower or raise **Reflections**, **Reverb** and **Room size**, or try *Natural* or *Gaming · Neutral*. |
| **Plugging in headphones doesn't switch** | With **Play on** set to *Automatic*, Aether follows the Windows default device within about 3 seconds. If you picked a device manually, it stays on that device. |
| **Control panel won't open** | Left-click the tray icon, or run `aether.exe` again; a second launch just opens the window. It's also reachable at <http://127.0.0.1:47820/>. |
| **Tray icon is missing** | Click the **^** near the clock. If it isn't there either, Aether isn't running. |

---

## Turning Aether off or uninstalling

**Temporarily:** flip the **On/Off** switch. Audio still flows through Aether but is passed through untouched.

**Fully, keeping it installed:**

1. Tray icon → **Quit**.
2. In Windows Sound settings, set your output back to your real headphones or speakers.

**Uninstall:**

1. Untick **Start with Windows**, then **Quit** Aether.
2. Set the Windows output back to your real device.
3. Delete `aether.exe` (and the project folder, if you built it).
4. Delete your settings and profiles folder: `%APPDATA%\Aether`.
5. Optional: uninstall VB-Audio Virtual Cable by running its setup again as administrator and choosing **Remove Driver**.
