Koharu fork for translating manga into Thai (based on Koharu 0.61.2).

**Install:** download `Koharu_…_x64-setup.exe` below and run it. Windows may
show a SmartScreen warning because the installer is not code-signed: click
"More info" → "Run anyway". The portable `.exe` runs without installing.

**Added in this fork**
- Characters: profiles, per-character font / colour / size / bold / italic /
  alignment / outline, speech cues (can be drafted from confirmed lines).
- Speakers: choose per block (keys 1–9 / 0 unknown / O other), guess automatically (uses block positions,
  the previous page and confirmed lines as examples), "Other (not in list)",
  Guess speakers as a pipeline step.
- Translation: character profiles, glossary and previous-page context sent
  to the LLM; translate only selected pages; re-translate one block with a
  request.
- Glossary with "Suggest terms"; Library of characters / terms shared by all
  projects (export / import as a file).
- Typesetting: project defaults (font, size scaled to the page, colour,
  outline), Thai dictionary line breaking (names and particles kept
  together, switchable), floating Thai tone marks fixed, text kept inside
  its block (or spread over its bubble, switchable), rotation handle,
  Reset style.
- Check panel (glossary misses, inconsistent lines, untranslated blocks).
- Colour pickers: "Pick" takes a colour from the page (the system eyedropper
  does not work in the app).
- Import pages from PDF / ZIP / CBZ / RAR / CBR (pages with a colour
  profile such as Apple RGB are converted to sRGB, so renders keep the
  original colours; pages of older projects too, after re-running inpaint
  and render); export PSD for all or selected pages (rotation kept);
  delete several blocks at once.
- Local LLMs (LM Studio etc.): replies are length-limited and runaway
  repeated sounds are cut, so a model looping on moans no longer runs on.
- Undo / redo also covers character and speaker edits.
