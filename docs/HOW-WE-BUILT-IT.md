# How it was built

Version 2.0 adds the FLOW 8 settings that Behringer only exposes in its phone app. None of them are in the
official MIDI chart, so the protocol had to be worked out from the outside. This is how, including what didn't
work. Protocol details are in [flow8-midi-implementation.md](./flow8-midi-implementation.md); tools are in
[`tools/`](../tools) (Linux/BlueZ, Python, no extra packages).

## 1. What MIDI can and can't do

The MIDI chart in Behringer's Quick Start Guide covers levels, EQ, sends, FX, snapshots and tap tempo, and
nothing else. Routing, phones source, USB modes, foot switch mode and output pads are app-only. The FLOW 8 also
**sends no MIDI at all** when you move its controls (checked on the raw USB MIDI port), so physical changes
can't be read over MIDI. The app talks to the mixer over a proprietary Bluetooth LE protocol (`FLOW 8 LE`).

## 2. A fake mixer on Linux: BLE pass-through ([`ble_proxy.py`](../tools/ble_proxy.py))

The first idea was a man-in-the-middle. The PC connects to the real mixer as a BLE client and, at the same time,
advertises itself as a `FLOW 8 LE` peripheral, built as a BlueZ GATT server over D-Bus. The phone app connects to the
PC; every packet is relayed both ways and logged with timestamps and markers you type in.

Making a convincing copy taught a few things:

* The real mixer's advertisement is exactly 31 bytes: the 128-bit service UUID plus 32-bit service data
  (`14839ad4 → 00 fd 06 2b 06 39 f1`), with the name `FLOW 8 LE` in the scan response.
* BlueZ encodes service data with a full 128-bit UUID unless you give it the short form. That makes the advertisement
  too long, so it silently becomes an *extended* advertisement, which most phones don't scan for.
* BlueZ won't put a name in raw scan-response data, so the copy puts the name and service data in the
  advertisement and the UUID in the scan response. Android merges both.
* Until a client authenticates, the mixer sends its identity packet every 0.5 s, and it drops an unauthenticated client after ~30 s.

The PC side worked: connected, advertising, relaying. But the mixer dropped the proxy before the phone arrived, and a
phone already paired with the PC may treat the copy as the PC. So we switched to a simpler route.

## 3. Capturing the phone app ([`btsnoop_att.py`](../tools/btsnoop_att.py))

Android can log every Bluetooth packet itself: Developer options → *Enable Bluetooth HCI snoop log* = **Enabled**,
toggle Bluetooth, use the app, then `adb bugreport`. Gotchas:

* Without that setting, the bugreport only contains `btsnooz_hci.log`, a ring buffer that keeps just the first
  bytes of each packet. That's not enough to see values.
* The *snoop log socket* option looks promising but isn't present on normal (user) Android builds.

With the full log, every app action shows up as a Bluetooth write. A generic settings record
(`25 01 id len value`, read with `26 01 id`), FX return routing (`11 01 bus …`) and app tap tempo (`40 01 bpm`)
fell out of one session.

## 4. Labelling the IDs

The settings IDs were matched to the app's switches by **reading** every ID from the mixer (`0x26`, read-only) and
comparing the answers with screenshots of the app's Routing and Preferences screens (`adb exec-out screencap`).

## 5. Finding the PHONES level in the state dump ([`dump_watch.py`](../tools/dump_watch.py))

A Bluetooth "dump" request (`4b 01 4c`) makes the mixer send its full 3068-byte state as SysEx over USB. Taking
dumps every few seconds while turning the PHONES knob and diffing them showed the level as a float (−144 = off
… +10 dB). If two programs read the USB MIDI port at once, each gets only part of the bytes, so the tool now
discards anything that isn't exactly 3068 bytes.

## 6. What went wrong, and the rules that came out of it

* A guessed Bluetooth write (`06 01 00 0f …`), hoped to be the headphones, was really the **Ch 1 fader** — but at
  the time it *looked* like it moved Main and several channels, because the desktop app and the test script were
  both reading the USB MIDI port and the dumps were corrupted. The levels were restored from a dump taken just
  before, using the official MIDI CC7 messages.
* An unlisted MIDI message (channel 8 CC21) changed a hidden Main-bus frequency setting. It was restored to within one MIDI step.
* Guessing MIDI CCs for the headphones found nothing for PHONES — and one unlisted CC (channel 8 CC21) silently
  changed a hidden setting. Guessing turned out to be the wrong approach; listening was the right one (next section).

Rules: **read before you write**, unplug speakers and headphones, change one thing at a time, take a dump before
and after every write, and stop at the first surprise.

## 7. Listening instead of guessing: live notifications and meters

Sending the phone app's subscribe message (`0x21`, copied verbatim) makes the mixer **push a message the moment a
hardware control moves** — the same `06 01 target param value` format the app writes. Moving one control at a time
mapped them: channel faders `00`–`06` (0-based), Main `0f`, **PHONES `06 01 09 09`**, FX MUTE (`08 …`), the bus
buttons (`41 01 bus`), and menu setting changes (`25 …`). Writing `06 01 09 09 VV` back then set the headphone level
with nothing else moving — that's the PHONES slider.

The same subscription starts the `0x22` meter stream. Making sound on one input at a time (the Ch 1 mic, then PC
playback over USB) showed the layout: one byte per mono strip, two per stereo strip, in strip order — that's the
live input meters.

## Credits

Directed and tested on the hardware by stemsee. The investigation, code and this write-up were done with
Claude Opus 5.5 (Anthropic) via Claude Code.
