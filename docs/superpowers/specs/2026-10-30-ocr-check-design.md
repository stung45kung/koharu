# ตรวจ OCR ด้วย LLM (`ocr-check`) — Design

วันที่: 2026-10-30 · สถานะ: รอผู้ใช้รีวิว

## เป้าหมาย

Manga OCR ยังอ่านตัวอักษรผิดบ่อย จึงเพิ่มขั้นตอนที่ให้ LLM ที่อ่านภาพได้ (Gemma ผ่าน LM Studio)
เทียบข้อความ OCR กับภาพของบล็อก แล้วแก้เฉพาะตัวอักษรที่อ่านผิด ก่อนส่งไปแปล

ผู้ใช้เลือกไว้:
- แก้ให้อัตโนมัติ แต่เก็บข้อความเดิม ติดป้ายบล็อกที่ถูกแก้ ดูของเดิมได้และคืนค่าได้
- LLM ต้องดูทั้งภาพครอปและข้อความ OCR (มี glossary และชื่อตัวละครช่วย)
- เป็นขั้นตอนใน pipeline ที่เปิด/ปิดได้ และมีปุ่มตรวจเฉพาะบล็อก

ถือว่าสำเร็จเมื่อ: เปิดสวิตช์แล้ว Process หน้า ได้บล็อกที่ OCR ผิดถูกแก้พร้อมป้าย
และคืนค่าได้ ส่วนบล็อกที่ OCR ถูกอยู่แล้วต้องไม่เปลี่ยน ถ้าไม่มี LLM หรือ LLM
อ่านภาพไม่ได้ pipeline ต้องทำงานต่อได้ตามปกติ

นอกขอบเขต: ตรวจแบบใช้แค่ข้อความ, ตรวจทั้งหน้าในภาพเดียว, ให้คะแนนความมั่นใจ

## 1. ข้อมูล (koharu-core)

- `TextData` เพิ่มช่อง `ocr_original: Option<String>` (`#[serde(default)]`):
  ข้อความ OCR ก่อน LLM แก้ มีค่าเฉพาะตอนที่ขั้นตรวจเปลี่ยนข้อความจริง
- `TextDataPatch` เพิ่ม `ocr_original: Option<Option<String>>` ให้ใช้ผ่าน
  `Op::UpdateNode` ได้ จึงกด Undo/Redo ได้เหมือนการแก้อื่น
- ไฟล์โปรเจกต์เก่าโหลดได้ เพราะช่องที่ไม่มีจะได้ค่า `None`

## 2. ขั้นตอน pipeline `ocr-check` (koharu-app)

ไฟล์ใหม่ `koharu-app/src/pipeline/engines/llm_ocr_check.rs` วางแบบเดียวกับ `speaker-guess`:

- `needs: [TextBoxes, OcrText]`, `produces: [OcrChecked]` artifact `OcrChecked` เป็นตัวใหม่
  ใช้จัดลำดับอย่างเดียว เหมือน `Speakers` คือ `ready()` คืนค่า `true` เสมอ จึงไม่ขวางขั้นไหน
- เพิ่ม `OcrChecked` ใน `needs` ของ `speaker-guess` และ `llm` (แปล) ถ้ามีขั้นนี้ในรอบที่รัน
  จะได้รันก่อนเดาผู้พูดและก่อนแปล
- ใน `step_for` ให้ `OcrChecked` ใช้ `PipelineStep::Ocr` เพื่อให้แถบเครื่องมือแสดงสถานะว่ากำลังทำ OCR
- บล็อกที่จะตรวจ: `target_text_nodes(&ctx)` (บล็อกที่ขอมา หรือทุกบล็อกในหน้า) เฉพาะที่มีข้อความ
- ครอป: ทั้งบล็อกด้วย `crop_text_block_bbox` → `shrink_for_reading` → ขยายภาพเล็กให้ด้านสั้นยาวอย่างน้อย 64px
  (ใช้ helper เดียวกับ `llm_vision_ocr`)
- ส่งให้ LLM ทีละบล็อก แต่ละบล็อกได้ instruction ของตัวเอง เพราะต้องแนบข้อความ OCR ของบล็อกนั้นไปด้วย
  จึงเพิ่ม `llm::Model::read_image(image, instructions)` สำหรับภาพเดียว
  (`read_images` เดิมยังใช้ได้เหมือนเดิม)
- Prompt (ภาษาอังกฤษ) บอกว่า: นี่คือ lettering จากมังงะ, ผล OCR คือ `<text>`,
  ให้เทียบกับภาพแล้วแก้เฉพาะตัวที่อ่านผิด/ตก/เกิน ห้ามแปล ห้ามเกลาสำนวน ห้ามเติมคำ
  ถ้าถูกแล้วให้ตอบข้อความเดิม ตอบแค่ข้อความเท่านั้น และแนบ "ชื่อที่อาจปรากฏ"
  = ชื่อตัวละครทุกคน + คำต้นทางใน glossary (สูงสุด 60 รายการ) ไม่ใช้
  `glossary.prompt_section` เพราะข้อความ OCR ที่อ่านผิดอาจจับคู่คำไม่เจอ
- ทำความสะอาดคำตอบ: ใช้ `clean_reply` ตัวเดียวกับ `llm_vision_ocr` (ย้ายไปไว้ใน
  `ocr_columns.rs` หรือ `support.rs` ให้ใช้ร่วมกัน)

### ตัวกันการแต่ง

ไม่รับคำตอบ (ใช้ข้อความเดิมและบันทึก `warn`) เมื่อ:
- คำตอบว่าง
- edit distance ระดับตัวอักษร (Levenshtein, นับเป็น `char`) มากกว่า 50% ของความยาวที่ยาวกว่า
- คำตอบยาวกว่าเดิมเกิน 2 เท่า + 4 ตัวอักษร

### ผลลัพธ์

- คำตอบหลังทำความสะอาดเท่ากับข้อความเดิม → ไม่สร้าง op
- ต่างกันและผ่านตัวกัน → `UpdateNode` ตั้ง `text = คำตอบ` และตั้ง
  `ocr_original = Some(ข้อความเดิม)` แต่ถ้าบล็อกมี `ocr_original` อยู่แล้ว
  จะเก็บค่าเดิมไว้ ป้ายจึงยังชี้ไปที่ผล OCR ตัวแรกสุด

### กรณีผิดพลาด

- ไม่มี LLM (`!ctx.llm.ready()`) → ข้าม (`info`) คืนค่า `[]`
- LLM ภายในเครื่องอ่านภาพไม่ได้ หรือ provider ตอบ error → บันทึก `warn` แล้วข้ามบล็อกนั้น/หน้านั้น
  ไม่ทำให้ทั้งหน้าล้มเหลว (เหมือน `speaker-guess`)
- ถ้ากดยกเลิก (`ctx.cancel`) ระหว่างทาง → หยุดแล้วคืน op ที่ทำเสร็จไปแล้ว

## 3. ล้างค่าเมื่อ OCR ใหม่

engine OCR ทุกตัว (`manga_ocr`, `mit48px_ocr`, `paddle_ocr`, `llm_vision_ocr`,
`llm_vision_page_ocr`) ต้องตั้ง `ocr_original: Some(None)` ใน patch ด้วย ถ้า OCR ใหม่
ป้ายเก่าจะหายไป แนะนำให้ทำ helper กลางใน `support.rs` เช่น `ocr_text_op(page, id, text)`
แล้วให้ทุกตัวเรียกใช้

## 4. UI

- `ui/lib/ocrBlocks.ts`: เพิ่ม `checkOcrBlocks(pageId, nodeIds)` → `startPipeline({ steps: ['ocr-check'], … })`
- `preferencesStore`: เพิ่ม `ocrCheck: boolean` (ค่าเริ่มต้น `false`)
- `MenuBar.tsx`: ใส่สวิตช์ "Check OCR with LLM" ไว้ในเมนู Process ถ้าเปิด จะใส่
  `'ocr-check'` ต่อจาก `p.ocr` ทั้งใน `runPipeline` และ `runCustomPipeline`
  ใน custom pipeline จะใส่ทุกครั้งที่สวิตช์เปิด ไม่ว่าจะเลือก OCR ไว้หรือไม่ เช่น ใช้ตรวจหน้าที่ OCR ไว้แล้ว
  เพราะขั้นนี้ข้ามบล็อกที่ไม่มีข้อความอยู่แล้ว
- `TextBlocksPanel.tsx` (`BlockCard`):
  - ปุ่ม "Check OCR" (ไอคอน `ScanSearchIcon`) ข้างปุ่ม OCR ปิดไว้เมื่อกำลังประมวลผลหรือยังไม่มี LLM
  - ถ้า `ocrOriginal` มีค่า: แสดงป้าย "LLM fixed" ชี้แล้วเห็นข้อความเดิม และมีปุ่มคืนค่า
    ที่ส่ง patch `{ text: ocrOriginal, ocrOriginal: null }`
- ข้อความ UI: เพิ่ม key ใน `en-US/translation.json` (ภาษาอื่นใช้ fallback)
- ชนิดข้อมูล API ฝั่ง UI ที่ generate มา (`TextData`/`TextDataPatch`) ต้องมี `ocrOriginal` ด้วย

## 5. การทดสอบ

Rust (unit test ในไฟล์ engine/core):
- ตัวกันการแต่ง: รับการแก้ 1–2 ตัวอักษร ไม่รับคำตอบว่าง ไม่รับคำตอบที่ต่างกันเกินครึ่ง ไม่รับคำตอบที่ยาวเกิน
- สร้าง op: ถ้าเหมือนเดิมไม่สร้าง op, ถ้าแก้จะได้ `ocr_original` เป็นข้อความเดิม,
  ถ้ามี `ocr_original` อยู่แล้วจะเก็บค่าเดิมไว้
- prompt มีข้อความ OCR, ชื่อตัวละคร และคำใน glossary โดยตัดที่ 60 รายการ
- `TextData` เก่าที่ไม่มีช่องนี้ deserialize ได้
- helper OCR ตั้ง `ocr_original` เป็น `None`

UI (vitest):
- เห็นป้ายเมื่อมี `ocrOriginal` กดคืนค่าแล้วส่ง patch ถูกต้อง
- ปุ่ม Check OCR เรียก `startPipeline` ด้วย `['ocr-check']` และบล็อกนั้น

## 6. Build / ส่งมอบ

push ขึ้น `stung45kung/koharu` แล้วรอ CI (lint/test/build Windows) ผ่าน จากนั้นออก
release ทดลองเวอร์ชันถัดไปทันทีตามขั้นตอนเดิมของ fork
