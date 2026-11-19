# Vutric — to'liq mahsulot dizayn topshirig'i (v2, kelajak uchun)

> Claude Design uchun. Avvalgi `BRIEF.md` faqat bitta rasm otishni qamragan. Bu topshiriq mahsulot **to'liq**
> bo'lganda kerak bo'ladigan BARCHA ekranlarni qamraydi: har xil ulanish yo'llari, ko'p fayl, saqlash
> sozlamalari, telefon va kompyuter. Brend, ranglar va shrift o'zgarmaydi (7-bo'lim).

## 1. Mahsulot bir gapda

**Nima qiladi.** Vutric o'z qurilmalaring orasida istalgan faylni (rasm, video, papka, hujjat, istalgan format,
istalgan hajm) bir zumda o'tkazadi. Yo'nalishlar:

- telefon ⇄ kompyuter;
- telefon ⇄ telefon;
- kompyuter ⇄ kompyuter.

**Qaysi yo'l bilan.**

- Bir Wi-Fi'da bo'lsa — to'g'ridan-to'g'ri.
- Alohida tarmoqlarda — internet orqali, shifrlangan.
- Tarmoq umuman yo'q bo'lsa — hotspot, Bluetooth yoki kabel orqali.

**Imzo-effekt.** Rasmni telefonda yuqoriga otasan — u kompyuter ekraniga halqadan uchib tushadi.

## 2. Dizayn tamoyillari (buzilmas)

1. **Har ekranda bitta asosiy harakat.** "Yuborish" har doim pastki uchdan birida, bosh barmoq yetadigan joyda turadi.
2. **Texnika yashirin, holat ochiq.** Foydalanuvchi "QUIC", "relay", "mDNS" so'zlarini ko'rmaydi. U faqat
   **yo'l nishoni**ni (3-bo'lim) va **jonli tezlik**ni ko'radi.
3. **Halol raqam.** Faqat o'lchangan tezlik ko'rsatiladi (MB/s, qolgan vaqt), va'da qilingan tezlik hech qachon
   yozilmaydi. "AirDrop'dan tez", "100% offline" kabi matn bo'lmaydi.
4. **Sukut — Avtomatik.** Yo'l tanlash bor, lekin oddiy foydalanuvchi uni hech qachon ochmasligi ham mumkin.
5. **Xato barmoq yonida, katta va harakati bilan.** Har bir xatoda nima bo'lgani bir gapda yoziladi va bitta tugma
   beriladi (qayta urinish yoki boshqa yo'l). Tepada mayda qizil matn bo'lmaydi.
6. **Fayl hech qachon "yarim" ko'rinmaydi.** Tayyor degani tekshirilgan degani. ✓ belgisi faqat oxirida chiqadi.
7. **Ishonch qurilma darajasida.** Birinchi marta QR skanerlanadi, keyin qurilma ro'yxatdan bir bosish bilan
   tanlanadi.
8. **Kirish imkoniyati.**
   - Reduced motion hurmat qilinadi: otish o'rniga oddiy tugma bor.
   - Har bir tugma kamida 44 pt.
   - VoiceOver va TalkBack uchun matnlar bor.
   - Dynamic Type 200% gacha kattalashganda ham ekran buzilmaydi.

## 3. Yo'l nishoni (route badge) — barcha ekranlarda bitta komponent

| Nishon matni | Qachon | Ikonka g'oyasi | Rang |
|---|---|---|---|
| To'g'ridan-to'g'ri · LAN | bir Wi-Fi | ikki nuqta, to'g'ri chiziq | Signal 500 |
| To'g'ridan-to'g'ri · internet | turli tarmoq, to'g'ridan ulanish chiqdi | chiziq + globus | Signal 500 |
| Relay orqali | to'g'ridan ulanish chiqmadi | chiziq o'rtasida kichik tugun | Glow 200 fon, Cobalt matn |
| Hotspot | telefon o'z tarmog'ini ochgan | Wi-Fi to'lqini + telefon | Cobalt |
| Bluetooth | faqat topish yoki kichik matn | BT belgisi | Cobalt |
| Kabel | USB | vilka | Cobalt |
| Yaqin (Wi-Fi Aware / Direct) | yaqin, routersiz | ikki to'lqin to'qnashuvi | Cobalt |

- Nishon yonida jonli tezlik turadi: "· 38 MB/s".
- Nishon bosilsa kichik varaq ochiladi: bu qaysi yo'l, nega aynan shu tanlangan va "Boshqa yo'l" tugmasi.

## 4. Telefon ekranlari

393×852 pt (iPhone 16 Pro), Android ham. Har biri Light (sand) VA Dark (night) temada.

### P1. Onboarding (3–4 kadr)

1. Salom: logo, bitta gap, "Boshlash" tugmasi.
2. Qurilma nomi (tahrirlanadi, masalan "Aziz iPhone'i").
3. Ruxsatlar — bittalab, har biri nega kerakligi bilan:
   - Mahalliy tarmoq: nega kerak va rad etilsa nima bo'ladi;
   - Kamera: QR uchun;
   - Galereya: tanlash va saqlash uchun;
   - Bildirishnoma;
   - Bluetooth: yaqindagi qurilmani topish uchun.
4. Birinchi qurilmani ulash: "Kompyuteringizda Vutric'ni oching", keyin "QR skanerlash" yoki "QR'imni ko'rsatish".

### P2. Bosh ekran — qurilmalar ro'yxati (ENG MUHIM)

- **Tepa qism:** logo, qurilma nomi, sozlamalar ikonkasi.
- **Qurilma kartasi:**
  - nomi va platforma ikonkasi (Windows / Mac / Linux / iPhone / Android);
  - holat nuqtasi: yashil — hozir yetib boriladi, sariq — faqat relay orqali, kulrang — offline;
  - yo'l nishoni;
  - oxirgi o'tkazma.
- **Kartadagi harakatlar:** tez "Yuborish" tugmasi bor; kartaning o'zi bosilsa qurilma tafsiloti (P9) ochiladi.
- **Past qism:** katta "Yuborish" (asosiy) va "Qabul qilish" tugmalari; "+ Yangi qurilma" (QR).
- **Holatlar:**
  - bo'sh — hali qurilma yo'q: illyustratsiya va "Ulash" tugmasi;
  - 1 ta qurilma;
  - 5 tadan ko'p qurilma;
  - hammasi offline;
  - internet yo'q — faqat yaqin yo'llar qoladi;
  - "Mahalliy tarmoqqa ruxsat berilmagan" banneri.

### P3. Yuborish varag'i (bottom sheet)

- **Manba tanlash:**
  - Galereya — ko'p tanlash, rasm va video;
  - Fayllar — istalgan format;
  - Papka;
  - Matn yoki havola;
  - Clipboard.
- **Tanlangandan keyin:**
  - tanlanganlar to'plami (eskizlar stack ko'rinishida, "+12");
  - jami hajm;
  - qabul qiluvchi qurilmalar chip'lari (bir nechtasini tanlash mumkin);
  - yo'l: "Avtomatik ▾".
- **Asosiy tugma:** "Yuborish · 24 fayl · 1.3 GB".
- **Muqobil:** tanlangan bitta rasm bo'lsa, uni yuqoriga otish.

### P4. Yo'l tanlash varag'i

**Yo'llar:**

- Avtomatik (tavsiya);
- Faqat shu tarmoq;
- Internet orqali;
- Tarmoqsiz (Hotspot);
- Bluetooth — faqat topish va kichik matn;
- Kabel;
- Yaqin (Wi-Fi Aware) — faqat mos qurilmalarda.

**Har bir qatorda:**

- bir gaplik izoh;
- halol tezlik diapazoni (masalan, Hotspot: "telefonga qarab ~2–50 MB/s");
- yo'l mavjud bo'lmasa, nega yo'qligi (masalan, iPhone'da: "iPhone hotspot ocha olmaydi — uni Android telefon ochadi").

### P5. O'tkazish jarayoni

Yuborish va qabul qilish uchun bitta komponent.

- **Ko'rinish:**
  - tepada qurilma va yo'l nishoni;
  - katta umumiy progress (%), ostida: "12 / 24 fayl · 640 MB / 1.3 GB · 38 MB/s · ~18 s qoldi";
  - fayllar ro'yxati: har faylda mini progress va holati — ✓ tayyor, ⟳ davom ettirilmoqda, ✕ o'tmadi.
- **Tugmalar:** Pauza · Bekor qilish.
- **iOS'da fonga o'tsa:** "Ilovani ochiq qoldiring" ogohlantirishi chiqadi.
- **Holatlar (har biri alohida kadr):**
  - javob kutilmoqda;
  - rad etildi;
  - aloqa uzildi → "Davom ettirish" (to'xtagan joyidan);
  - relay'ga o'tdi: nishon o'zgaradi, tezlik pasayadi, sababi tushuntiriladi;
  - disk to'ldi;
  - hammasi tayyor: yashil yakun "24 fayl · 1.3 GB · 34 s · 18 tasi galereyada" va "Papkani ochish" tugmasi.

### P6. Kiruvchi taklif

- Katta varaq yoki to'liq ekran:
  - kim yubormoqda (qurilma nomi va tekshiruv kodi);
  - nechta fayl va qancha hajm;
  - rasm bo'lsa eskizlar;
  - birinchi 5 ta fayl nomi.
- **Tugmalar:** "Qabul qilish" · "Rad etish".
- **Saqlash joyi:** pastda "Qayerga: Fayllar → Vutric ▸"; bosilsa saqlash joyini shu yerning o'zida o'zgartirish
  mumkin.
- **Belgi:** "Bu qurilmadan doim avtomatik qabul qilish".
- **Ishonchsiz qurilmadan kelsa:** ogohlantiruvchi ko'rinish.

### P7. Rasm otish (imzo-effekt)

- Eski `BRIEF.md` S3 dagi barcha holatlar saqlanadi.
- **Yangi:** ko'p rasm tanlansa kartalar to'plami ko'rinadi. Yuqoriga otilganda birinchi karta uchadi, qolganlari
  hisoblagich bilan ergashadi ("+11").

### P8. Qabul qilish / kutish ekrani

- Katta "Mening QR'im".
- **Ko'rinish rejimi:**
  - sukut — faqat ishonchli qurilmalar;
  - "Yaqin atrofda hammaga ko'rinish: 10 daqiqa" — taymerli, vaqt tugasa o'zi o'chadi.
- **Kelgan rasm** ekran tepasidan uchib kiradi (eski S4).

### P9. Qurilma tafsiloti

- **Ma'lumot:** nomi (o'zgartirsa bo'ladi), platforma, tekshiruv kodi, qachon juftlangani, oxirgi yo'l, o'rtacha tezlik.
- **Shu qurilma uchun alohida sozlamalar:** saqlash papkasi, avtomatik qabul, galereyaga saqlash.
- **Shu qurilma bilan o'tkazmalar tarixi.**
- **"Ishonchni olib tashlash"** — xavfli harakat, tasdiq so'raladi.

### P10. Tarix

- **Yozuvlar** kunlar bo'yicha guruhlangan. Har bir yozuvda:
  - yo'nalish: ↑ yuborildi / ↓ keldi;
  - qurilma;
  - fayllar soni va hajmi;
  - yo'l;
  - holat.
- **Filtrlar:** qurilma, fayl turi (rasm / video / hujjat), muvaffaqiyatsizlar.
- **Harakatlar:** qayta yuborish, faylni ochish.

### P11. Sozlamalar (asosiy)

Guruhlar telefonda ham, kompyuterda ham **bir xil tartibda**:

1. **Saqlash** → P12.
2. **Qabul qilish:**
   - har doim so'ralsin / ishonchli qurilmalardan avtomatik / N MB gacha avtomatik;
   - qabul qilingandan keyin: bildirishnoma / faylni ochish / papkani ochish.
3. **Ulanish yo'llari:**
   - sukut yo'l (Avtomatik…);
   - "Faqat shu tarmoq" kaliti (internetga hech narsa chiqmaydi);
   - Bluetooth orqali topish;
   - hotspot rejimi;
   - o'z relay manzili (ilg'or).
4. **Qurilmalar:** ro'yxat, yangi qurilma qo'shish.
5. **Maxfiylik:**
   - ko'rinish (faqat ishonchlilar / 10 daqiqa hammaga);
   - tarix muddati;
   - anonim statistika (ixtiyoriy).
6. **Ko'rinish:** tema (avto / yorug' / qorong'i), effekt (Halqa…), haptic, harakatni kamaytirish.
7. **Ilg'or:** texnik o'lchov (p50/p95), loglar, versiya.

### P12. Saqlash sozlamalari (yangi, MUHIM)

- **Saqlash papkasi.** Joriy yo'l va "O'zgartirish" tugmasi (tizim papka tanlagichi). iPhone'da Fayllar, iCloud Drive
  yoki USB disk tanlanadi, Android'da istalgan papka.
- **Turlar bo'yicha ajratish.** Kalit yoqilsa 6 qator ochiladi: Rasmlar / Videolar / Hujjatlar / Arxivlar / Audio /
  Boshqa. Har biriga o'z papkasi tanlanadi.
- **Ichki papka.** Variantlar: yo'q / qurilma bo'yicha / sana bo'yicha / qurilma + sana. Ostida jonli namuna yo'li
  ko'rinadi: "Vutric / Aziz-PC / 2026-11-18 / IMG_001.jpg".
- **Galereyaga saqlash (telefon).** Kalit, albom nomi va "Papkada ham nusxa qolsin" belgisi.
- **Papka tuzilmasi.** Yuboruvchidagi tuzilma saqlansin yoki tekislansin.
- **Nom to'qnashuvi.** Variantlar:
  - ikkalasi ham qolsin (`nom (2).jpg`);
  - almashtirilsin;
  - o'tkazib yuborilsin;
  - har safar so'ralsin.
- **Bo'sh joy himoyasi.** Slider: "1 GB dan kam joy qolsa qabul qilinmasin".
- **Tugallanmagan qismlar.** Davom ettirish uchun 7 kun saqlanadi; yonida "Hozir tozalash (240 MB)" tugmasi.
- **Holat:** tanlangan papka yo'qolsa yoki ruxsati tugasa, "Fayllar standart papkaga saqlandi" banneri chiqadi.

### P13. Hotspot oqimi

- **Android (tarmoqni ochuvchi):**
  1. "Tarmoqsiz ulanish" tanlanadi.
  2. Telefon o'z tarmog'ini ochadi.
  3. Katta QR ko'rinadi: tarmoq, parol va qurilma.
  4. "Ulanishni kutyapman".
  5. Ulandi.
- **iPhone (ulanuvchi):**
  1. QR skanerlanadi.
  2. Tizimning "Wi-Fi'ga ulanish" so'rovi chiqadi; uning yonida tushuntirish beriladi.
  3. Ulandi.
  4. O'tkazish boshlanadi.
- **Xato:** ba'zi Xiaomi va Huawei telefonlari hotspot ocha olmaydi. Bunda ekranda muqobil yo'llar taklif qilinadi.

### P14. Bluetooth bilan topish

1. "Yaqin atrofdagi qurilmalar" — radar yoki ro'yxat, tinch animatsiya bilan.
2. Topilgan qurilma bosiladi.
3. Ikkala ekranda 6 xonali kod solishtiriladi.
4. Ulandi. Shundan keyin Wi-Fi yo'li o'zi tanlanadi va bu foydalanuvchiga ko'rsatiladi.

### P15. Kabel rejimi

- "Telefonni kabel bilan ulang" illyustratsiyasi.
- **Holat:** kabel topildi / ruxsat kerak / ulandi.
- **Tezlik.**

### P16. Xato va chekka holatlar

Har biri alohida kadr:

| Holat | Ekranda nima taklif qilinadi |
|---|---|
| Mahalliy tarmoqqa ruxsat yo'q (iOS) | "Sozlamalarni ochish" |
| Router qurilmalarni bir-biridan ajratadi (mehmonxona Wi-Fi) | "Internet orqali" yoki "Hotspot" |
| Internet ham, umumiy tarmoq ham yo'q | yaqin yo'llar |
| Relay sekin (limit bor) | halol tushuntirish |
| Qabul qiluvchi offline | "U onlayn bo'lganda yuborish" (navbat) yoki shifrlangan havola (ixtiyoriy) |
| Disk to'ldi | — |
| Fayl buzildi | fayl o'zi qayta yuboriladi |
| Qabul qiluvchi rad etdi | — |

### P17. Tizim integratsiyasi

- **Share sheet (iOS/Android):** boshqa ilovadan Vutric orqali yuborish. Kichik varaq ochiladi, unda qurilma tanlanadi.
- **Bildirishnomalar:**
  - kiruvchi taklif (Qabul / Rad);
  - jarayon (Android'da doimiy bildirishnoma);
  - tayyor.
- **Live Activity / Dynamic Island (iOS):** o'tkazish progressi (ixtiyoriy).
- **Widget:** sevimli qurilmaga tez yuborish.

## 5. Kompyuter ekranlari

Avval Windows 11, keyin macOS va Linux. Har biri Light VA Dark temada.

### D1. Tray / menyu paneli menyusi

- Qurilmalar (holat nuqtalari bilan).
- "Fayl yuborish…".
- "Telefonni ulash (QR)".
- "Qabul qilish: so'ralsin / avtomatik".
- "To'xtatib turish", "Sozlamalar", "Chiqish".
- Joriy o'tkazma bo'lsa, uning progressi shu menyuda ko'rinadi.

### D2. Asosiy oyna (kichik, taxminan 420×640)

- P2 ning kompyuter versiyasi: qurilmalar va so'nggi o'tkazmalar.
- Katta **sudrab tashlash maydoni**: "Fayl yoki papkani shu yerga tashlang".

### D3. Send Zone (ekran chetidagi tushirish zonasi)

- Fayl sudralganda ekran chetida (chap, o'ng yoki tepada) qurilmalar halqasi paydo bo'ladi.
- Faylni kerakli qurilma ustiga tashlash yetarli.
- Uslubi imzo-effekt bilan bir xil (`03-effekt/`).

### D4. Kiruvchi taklif (toast va kengaytirilgan oyna)

- **Windows toast:** "Aziz iPhone'i 12 ta fayl yubormoqda (340 MB)" · Qabul · Rad · Qayerga ▸.
- **Tayyor toast:** "Ochish" · "Papkada ko'rsatish".

### D5. O'tkazish oynasi

- P5 dagi ma'lumot, desktop zichligida.
- Bir nechta o'tkazma bo'lsa, ular parallel ro'yxatda ko'rinadi.

### D6. Overlay effekt (rasm halqadan uchib tushadi)

- **Mavjud effekt:** `03-effekt/kompyuterdagi-effekt.md`.
- **Yangi holatlar:**
  - ko'p faylda — stack va hisoblagich;
  - videoda — birinchi kadr;
  - hujjatda — fayl ikonkasi tushirilgan karta.

### D7. Sozlamalar oynasi

- P11 va P12 bilan **bir xil guruhlar va tartib**, chap tomonda navigatsiya paneli.
- **Saqlash bo'limi:** tizim papka dialogi, turlar bo'yicha papkalar, ichki papka namunasi.
- **Faqat kompyuterda bor:**
  - avtostart;
  - Send Zone qaysi tomonda turishi;
  - effekt davomiyligi;
  - Explorer'dagi o'ng tugma menyusida "Send with Vutric".

### D8. QR juftlash oynasi

- Katta QR, qurilma nomi, tekshiruv kodi.
- Ko'rsatma: "Telefonda Vutric → Yangi qurilma".

## 6. Umumiy komponentlar (bitta tizim)

**Komponentlar:**

- qurilma kartasi;
- holat nuqtasi;
- yo'l nishoni (tezlik bilan);
- fayl qatori: eskiz yoki ikonka, nom, hajm, mini progress, holat;
- fayllar to'plami (stack);
- progress bloki;
- natija va xato pill'i;
- taklif varag'i;
- sozlama qatori: kalit, tanlov, slider, yo'l tanlagich;
- bo'sh holat illyustratsiyasi;
- xavfli harakat uchun tasdiq dialogi.

**Fayl turi ikonkalari:** rasm, video, PDF, Word, Excel, arxiv, audio, APK/EXE, papka, boshqa.

## 7. Brend (o'zgarmaydi — `02-brend/`)

- **ds-screens.png dagi A1–A6 — asosiy uslub namunasi.** Yangi ekranlar shu oilaga mos tushsin.
- **Ranglar:**

  | Nom | Kod |
  |---|---|
  | Cobalt 900 | `#062B8A` |
  | Signal 500 | `#2F6BFF` |
  | Glow 200 | `#F4D9A6` |
  | Sand 200 | `#E6DBC7` |
  | Sand 50 | `#F7F2E9` |
  | Night 950 | `#040C29` |
  | OK | `#1D7A4E` |
  | Xato | `#A8321F` |

  **Binafsha rang ishlatilmaydi.**
- **Shrift:** Geist / Geist Mono.
- **Radius:** 8 / 14 / 22 / to'liq. Tugma balandligi 44+.
- **Matn:** o'zbek lotin, apostrof `ʼ` ("koʻrinadi", "oʻtkazish").

## 8. Harakat (motion)

- Otish 200–300 ms davom etadi, qaytishda easeOutBack, effekt — halqa (`03-effekt`).
- Progress silliq yursin, sakramasin. Tezlik raqami soniyasiga bir marta yangilanadi.
- Yo'l nishoni almashganda (relay → to'g'ridan-to'g'ri) yumshoq morf bilan o'tadi.
- Reduced motion yoqilsa, barcha harakat o'chadi va holat matn bilan beriladi.

## 9. Topshiriladigan natija

1. **Ekranlar:** telefon P1–P17 va kompyuter D1–D8. Har biri yuqoridagi **barcha holatlari** bilan, Light va
   Dark temada.
2. **Komponentlar sahifasi** (6-bo'lim): o'lchamlar, padding, radius, shrift o'lchamlari — Flutter'ga ko'chirish uchun.
3. **Motion izohlari** (8-bo'lim): yuborish, qabul qilish, yo'l almashishi va xato oqimlari uchun.
4. **Chizish tartibi (ustuvorlik bo'yicha):**
   1. P2;
   2. P3;
   3. P5;
   4. P6;
   5. P12;
   6. P11;
   7. D2, D4, D7;
   8. P4;
   9. P9, P10;
   10. P13–P17;
   11. P1.
