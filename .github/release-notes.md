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
  together, switchable), floating Thai tone marks fixed, narrow boxes
  widened, text kept inside its block (switchable), rotation handle,
  Reset style.
- Check panel (glossary misses, inconsistent lines, untranslated blocks).
- Import pages from PDF / ZIP / CBZ / RAR / CBR; export PSD for all or
  selected pages (rotation kept); delete several blocks at once.
- Undo / redo also covers character and speaker edits.
