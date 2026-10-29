# Legal and policy notes

**This is not legal advice.** It is a summary of the facts about what this
project does and does not contain, plus the issues worth checking with a lawyer
or the vendor if you plan to redistribute it.

## What this project does

- Observes USB traffic between the vendor application and the keyboard.
- Implements an independent driver and CLI from those observations.
- Implements an independent `hidapi.dll` that exposes the public hidapi ABI so
  the vendor application can run under Wine.
- Documents the wire protocol for interoperability.

It does **not**:

- download, modify or flash firmware;
- bypass an authentication, encryption or copy-protection mechanism;
- include or link against vendor source code;
- patch, wrap or redistribute the vendor executable itself.

## What is not distributed

| item | status |
|---|---|
| `PhantomS Software.exe` and its DLLs | not included |
| original vendor `hidapi.dll` | not included (only an independently written replacement is built) |
| firmware images | not included |
| vendor `Config.json`, `LedInfo.xml`, layout XMLs | not included; read from your own installation |
| video frames/audio (including any Bad Apple copy) | not included |
| photos used for XY mapping | not included (only the resulting coordinates) |
| generated device state and mappings | included; these are data produced by the tools |

The original vendor XML files were used during development but have been
removed from the repository. To use layout-dependent scripts, point them at
`program_files_x86/xml/...` inside your own vendor installation.

## Vendor EULA status (checked against the official installer)

The current official download (`PhantomS_Software 06172025.zip`, linked from
the Phantom S product page) was inspected:

- It is a 34 MB NSIS installer. The visible wizard is: language selection ->
  destination folder -> progress -> finish. **No license or agreement page is
  shown.**
- The installer does contain `$PLUGINSDIR/license.txt` and a skin
  `licensepage.xml`, but the page is marked `visible="false"` and is not part
  of the flow. The text is a sample from the NSIS skin vendor (乖乖牛 /
  `ggniu.cn`), titled `《牛牛截图示例用户许可协议》`, and it explicitly calls
  itself a test/sample agreement. It is not a Tecware agreement.
- The installed application contains no EULA, license, terms or agreement
  files, and no such strings.
- `tecware.co` has no EULA, terms or privacy page in its sitemap, and the
  usual URLs (`/terms`, `/privacy`, `/warranty`, `/legal`, ...) return 404.
  Software is distributed as direct archive links from product pages.

So as of this check, **no Tecware EULA was presented by the installer or the
application**, and none is published on the vendor site. That does not remove
copyright protection from the software or its data files, and hardware
warranty/sales terms may be separate. It mainly means there is no
software-license contract text to point at.

## Reverse engineering and EULAs

Many jurisdictions provide some protection for reverse engineering performed
for interoperability, for example:

- United States: 17 U.S.C. §1201(f) (interoperability exception to the DMCA
  anti-circumvention provisions).
- European Union: Directive 2009/24/EC, Article 6 (decompilation for
  interoperability), subject to conditions.

These rules are not uniform and do not automatically override a contract. The
vendor application's EULA may contain a clause prohibiting reverse
engineering. Whether such a clause is enforceable depends on jurisdiction,
the specific wording, and the facts. If you accepted that EULA, read it; this
project does not claim any rights under it and does not grant you any.

This project did not decompile the vendor application or firmware. The protocol
was derived from observed traffic, probing, and the vendor's own data files.

## Vendor data files

Values such as mode IDs, field offsets, usage pages and checksum rules are
facts about the interface. The original files that contained them are not
redistributed. The documentation quotes short identifiers
(`light_mode_wave`, `RGB-87K-Keyboard-KF010.xml`) only to describe
compatibility.

If you plan to publish a fork, do not re-add vendor configuration files,
layout XMLs, DLLs or firmware. Have users extract what they need from their own
installation.

## hidapi ABI and third-party code

`bridge/shim.c` implements the public hidapi ABI because the vendor
application imports those symbols. The ABI and the `hid_device_info` layout
originate from the hidapi project, which is distributed under a BSD-style
license (see the hidapi repository for the exact text). The shim's
implementation is independent, but if you redistribute the shim, keep the
hidapi attribution and license in mind.

Other dependencies (Rust crates, Wine, mingw-w64, ImageMagick, mpv/ffplay) are
used under their own licenses. None are vendored here.

## Trademarks

"Tecware", "Phantom S" and "I-CHIP" are trademarks of their respective owners.
They are used here only to describe what the hardware is and what this
software is compatible with. There is no affiliation with or endorsement by
the vendor. Do not use vendor logos or imply endorsement.

## Media rights

Bad Apple!! and any other video or audio used with the player are the property
of their rightsholders. No frames or audio are distributed. The player reads a
local copy you provide.

## Risk and warranty

The protocol is undocumented and some commands are still experimental
(`profile --set`, raw macro/FN-lock writes). Nothing here flashes firmware, but
an incorrect write can leave the board in an odd state. The documented
hardware reset (Fn + Esc + F1 + F3 + F5 for about 3 seconds) is the recovery
path. Use at your own risk; there is no warranty.

## Licensing this repository

There is currently no `LICENSE` file for the original code in this repository.
Before publishing, choose one (MIT or Apache-2.0 are common for this kind of
tool). A code license does not grant rights to vendor files, firmware,
trademarks or third-party media, and does not resolve any EULA question.

## Practical checklist before publishing

- No vendor binaries, firmware, DLLs or XML/config files in the tree.
- No video/audio media or source photos.
- `docs/legal.md` kept with the project.
- A project `LICENSE` chosen and added.
- Keep the hidapi attribution in `bridge/` if the shim is redistributed.
- Make clear the project is unaffiliated and unsupported by the vendor.
