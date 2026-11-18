# Vutric telefon ilovasi — dizayn topshirig'i

## Mahsulot bir gapda
Telefondagi rasmni barmoq bilan **yuqoriga otasiz** — u bir lahzada kompyuter ekraniga **uchib tushadi**
(yoki boshqa telefonga). Internet ishlatilmaydi: faqat bitta Wi-Fi ichida, QR kod bilan juftlashadi.

## Platforma va cheklovlar
- Flutter, Material 3; asosiy qurilma **iPhone 16 Pro (393×852 pt)**, Android ham.
- Bir qo'l bilan ishlatish; asosiy tugmalar pastki uchdan birida.
- **Light (sand) va Dark (night)** — ikkala tema ham kerak.
- Matn: **o'zbek lotin**, apostrof `ʼ` (masalan "koʻrinadi").
- iPhone'da tarmoqdan avtomatik topish yo'q → **faqat QR** orqali ulanadi (Android'da "Tarmoqdan topish" ham bor).
- Haptic (otishda), `reduced motion` hurmat qilinadi, otish imo-ishorasidan tashqari **"Yuborish" tugmasi** ham bo'ladi
  (accessibility).

## Brend (02-brend/)
- `ds-colour.png`, `ds-type.png`, `ds-components.png`, `ds-effect.png` — tayyor dizayn-tizimi (https://vutric.wonmor.io).
- **`ds-screens.png` — rejadagi ekranlar A1–A6: ASOSIY USLUB NAMUNASI shu.** Yangi ekranlar shu oilaga tushsin.
- Ranglar: Cobalt 900 `#062B8A` (asosiy matn/tugma), Signal 500 `#2F6BFF` (aktiv, fokus), Glow 200 `#F4D9A6`
  (urg'u), Sand 200 `#E6DBC7` (fon), Sand 50 `#F7F2E9` (karta), Night 950 `#040C29` (qorong'i fon),
  OK `#1D7A4E`, Xato `#A8321F`. **Binafsha yo'q.**
- Shrift: Geist / Geist Mono (`02-brend/fonts`). Radius 8 / 14 / 22 / to'liq. Tugma balandligi 44+.
- Logo: `02-brend/logo.svg`. To'liq tokenlar: `02-brend/sayt-dizayn-tizimi.css` (`:root`).
- Kompyuterdagi effekt (rasm halqadan chiqadi): `03-effekt/`.

## Hozirgi holat va muammolar (01-hozirgi-ekranlar/)
1. `1-bosh-ekran.jpg` — texnik ro'yxat: 7 ta bir xil tugma, havola maydoni, QR pastda. Nima asosiy — ko'rinmaydi.
2. `2-otish-ulangan.jpg` — otilgan rasm **kompyuter nomi paneli ostiga kirib ketadi** (panel rasm ustida chiziladi).
3. `3-otish-xato.jpg` — xato tepada mayda qizil matn ("$1" — kod xatosi, tuzatildi); natija/xato ko'zga tashlanmaydi.
4. Svayp qattiq: faqat juda tez harakat otish hisoblanadi, rasm barmoqqa "yopishmaydi" (og'ish, kichrayish yo'q).

## Chizish kerak bo'lgan ekranlar va holatlar

### S1. Bosh ekran
- Tepada: logo + qurilma nomi (tahrirlanadi, masalan "iPhone").
- **Ikkita katta harakat:** "Rasm otish" (asosiy) va "Rasm qabul qilish".
- "Bu qurilmaning QR kodi" — boshqa qurilma skanerlashi uchun (yig'iladigan blok yoki alohida varaq).
- Yashirin "Texnik o'lchov" bo'limi (100 ta probe, natija jadvali: p50/p95 ms) — oddiy foydalanuvchi ko'rmaydi.
- Holatlar: tayyorlanmoqda · tayyor · ishga tushmadi (xato + qayta urinish).

### S2. QR skaner (ulanish)
- Namuna: A2. Matn: "Kompyuterdagi QR kodni skanerlang", ostida "Kompyuterda: vutric … --listen".
- Muqobil: havolani qo'lda kiritish (kichik matnli tugma).

### S3. Rasm otish — ENG MUHIM EKRAN (namuna: A5)
Holatlar (har biri alohida kadr):
1. **Ulanmagan** — "Kompyuterga ulaning" + "QR kodni skanerlash" (Android: + "Tarmoqdan topish").
2. **Ulanmoqda** — progress + "Bekor qilish".
3. **Ulangan, rasm yo'q** — tepada qurilma pill'i (●  XSTUDIO · Windows, tekshiruv kodi 000092),
   markazda katta "Rasm tanlang" maydoni.
4. **Rasm tayyor** — katta rasm kartasi markazda, ustida "↑ Yuborish uchun yuqoriga suring" ishorasi
   (yengil animatsiyali strelkalar), pastda "Boshqa rasm" + "Yuborish".
5. **Sudrash paytida** — karta barmoqqa ergashadi, yonga og'adi (rotate), yuqoriga ko'tarilganda biroz kichrayadi.
6. **Uchib ketdi / kutilmoqda** — karta ekran tepasidan **hamma narsaning ustidan** chiqib ketadi; pastda
   "Yuborilmoqda…" pill.
7. **Yetib bordi** — yashil pill "✓ Yetib bordi · 38 ms"; karta pastdan qaytib keladi; oxirgi 3–5 natija kichik qatorda.
8. **Rad etildi** — "Kompyuter rasmni qabul qilmadi" (sessiya saqlanadi, yana otsa bo'ladi).
9. **Javob yo'q (5 s)** / **Aloqa uzildi** — xato + "Qayta ulanish".
- Qoidalar: karta hech qachon panel ostiga kirmaydi; xato/natija pastda, barmoq yonida; bitta asosiy harakat.

### S4. Rasm qabul qilish
1. **Tanlov** — "Kompyuter QR kodini skanerlash" yoki "Kutish: QR kodimni skanerlashsin".
2. **Kutish** — katta QR markazda + "Bekor qilish".
3. **Ulangan, kutilmoqda** — qurilma pill'i + "Rasm kutilmoqda…" (tinch animatsiya).
4. **Rasm keldi** — rasm **ekran tepasidan** (otuvchi qo'yib yuborgan joydan, og'ish bilan) uchib kirib markazga
   tushadi; ostida "3 ta rasm keldi · ekranda 12 ms".
5. **Aloqa uzildi** — xato + qayta ulanish.

### S5. Umumiy komponentlar
- Qurilma pill'i (holat nuqtasi: yashil ulangan / kulrang yo'q), tekshiruv kodi.
- Natija/xato pill'i (ok / xato / jarayonda).
- Rasm kartasi (radius 22, soya, sand 50 ramka).

## Topshiriladigan natija
- S1–S4 barcha holatlar, **393×852**, Light va Dark.
- S3 uchun harakat (motion) izohi: sudrash, otish (≈200–300 ms), qaytish (easeOutBack), natija pill'i.
- Komponent o'lchamlari (padding, radius, shrift o'lchami) — Flutter'ga ko'chirish uchun.
