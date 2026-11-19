# Ikki qurilma bir-birini qanday topadi va fayl uzatadi: barcha ma'lum usullar

**Vutric uchun tadqiqot hisoboti** · 2026-11-18 · Qamrov: telefon ⇄ kompyuter, telefon ⇄ telefon, kompyuter ⇄ kompyuter (Android, iOS, Windows, macOS, Linux)

> **O'qish qoidasi.** Har bir raqam yonida `[N]` manba bor (5-bo'lim). Manba ishlab chiqaruvchining o'zi bo'lsa, **(vendor)** deb yozilgan. Forum yoki blogdagi bitta tajriba **(anekdot)**. O'zim tasdiqlay olmagan narsa **tekshirilmagan**. Raqam topilmagan joyda "raqam yo'q" deb yozilgan, taxmin qo'yilmagan. GitHub yulduzlari 2026-11-18 kuni GitHub API orqali olingan [102].

---

## 0. Qisqa xulosa

1. **Bir xil LAN** eng tez va eng oddiy yo'l. Vutric'da bu allaqachon bor (iroh + mDNS). Muammo tezlikda emas, **topishda**: multicast bloklanadi, AP isolation bo'ladi, iOS "Local Network" ruxsati so'raladi [30][98].
2. **Internet orqali** to'g'ridan-to'g'ri ulanish ko'p hollarda chiqadi: libp2p DCUtR o'lchovida 70% ± 7.1% [40], Tailscale ichki ko'rsatkichi "90% dan ancha yuqori" [43]. Qolgan holatlar uchun **relay shart**. Uyali tarmoqlarning 90% dan ortig'i CGNAT ishlatadi [46].
3. **Tarmoqsiz, yaqin masofada** kuchli usullar: Wi-Fi Aware (NAN), Wi-Fi Direct, Android LocalOnlyHotspot va kabel (USB/Ethernet). BLE faqat topish va kalit almashish uchun yaraydi. iOS'da amaldagi o'lchov 400–600 kbit/s atrofida (anekdot) [27].
4. **iOS eng tor joy.** Wi-Fi Direct API yo'q. Hotspot'ni dastur ochib bera olmaydi [34]. AWDL'ga faqat Apple framework'lari orqali kirish mumkin [11][31]. Wi-Fi Aware iOS 26 dan bor (iPhone 12+) [1], lekin Android bilan pairing **hozircha ishlagani isbotlanmagan** [7][9].
5. **Tavsiya etilgan tartib** (4-bo'lim):
   - LAN topishni mustahkamlash va o'z iroh-relay'ingizni ko'tarish;
   - hotspot rejimi (Android LOHS, QR bilan);
   - BLE faqat topish uchun;
   - USB kabel rejimi;
   - Wi-Fi Aware: avval bir platforma ichida, keyin cross-platform;
   - shifrlangan cloud-handoff (ixtiyoriy);
   - animatsion QR, kichik ma'lumot va air-gap holatlari uchun.

---

## 1. Katta taqqoslash jadvali

Belgilar: ✅ bor/ishlaydi · ⚠️ cheklangan · ❌ yo'q · ? noma'lum/tekshirilmagan. "Tezlik" ustunida manba ko'rsatgan **amaldagi** (yoki nazariy deb belgilangan) qiymat turibdi.

### 1a. Link qatlami va platforma API'lari (uchinchi tomon ilovasi ishlata oladigan narsalar)

| Usul | Qaysi vaziyat | Tezlik | Masofa | Android | iOS | Windows | macOS | Linux | Internet kerakmi | Ruxsat / entitlement | Ochiq/yopiq | Manba |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| Bir xil Wi-Fi/LAN (TCP/QUIC) + mDNS | Bir LAN | AP va qurilma imkoniyatiga bog'liq. LocalSend Wi-Fi 6E'da 24–42 MB/s (anekdot, ishonchliligi past) | AP qamrovi | ✅ (MulticastLock) | ✅ (Local Network ruxsati, multicast entitlement) | ✅ (firewall) | ✅ (macOS 15+ Local Network) | ✅ | Yo'q | iOS: `NSLocalNetworkUsageDescription`, `NSBonjourServices`, multicast uchun `com.apple.developer.networking.multicast` | Ochiq standart | [30][55][89] |
| Ethernet / Thunderbolt / USB4 to'g'ridan-to'g'ri kabel (link-local IPv6) | Tarmoqsiz, ikki kompyuter | Kabel tezligi. Raqam o'lchanmagan | Kabel | ⚠️ (USB-Ethernet adapter) | ⚠️ | ✅ | ✅ | ✅ | Yo'q | Odatiy tarmoq ruxsatlari | Ochiq | tekshirilmagan |
| Wi-Fi Aware (NAN) | Yaqin, tarmoqsiz. Isolated Wi-Fi'da ham ishlaydi (STA bilan birga) | Ditto: 100+ Mbit/s (Wi-Fi 5), 250+ Wi-Fi 6 (vendor, tekshirilmagan) | Ditto: ~50–100 m (vendor) | ✅ Android 8+, qurilmaga bog'liq. Xabar ~255 bayt | ✅ iOS 26+, iPhone 12+. Faqat **pairing qilingan** qurilmalar bilan, 6 xonali PIN | ❌ (ommaviy API topilmadi) | ❌ (iOS'da bor, macOS'da yo'q) | ⚠️ nl80211'da NAN buyruqlari bor, drayverga bog'liq | Yo'q | Android: `NEARBY_WIFI_DEVICES` (33+), avval `ACCESS_FINE_LOCATION`. iOS: `com.apple.developer.wifi-aware` + `WiFiAwareServices` | Ochiq standart (WFA) | [1][2][5][9][10][92] |
| Wi-Fi Direct (P2P) | Yaqin, tarmoqsiz | Galaxy A10s'da ~16–20 Mbit/s (o'lchov) [25]. Samsung↔Samsung 54.7 MB/s (anekdot) [99] | Wi-Fi qamrovi | ✅ | ❌ (API yo'q) | ✅ `Windows.Devices.WiFiDirect`. Mobile Hotspot bilan bir vaqtda ishlamaydi | ❌ (AWDL bilan mos emas) | ✅ wpa_supplicant (drayverga bog'liq) | Yo'q | Android: `NEARBY_WIFI_DEVICES` | Ochiq standart | [17][25][37] |
| Android LocalOnlyHotspot (softAP) | Tarmoqsiz, isolated Wi-Fi | Wi-Fi tezligi. Raqam o'lchanmagan | Wi-Fi qamrovi | ✅ Android 8+. SSID/parol tasodifiy, internet yo'q. Ba'zi MIUI/HarmonyOS'da ishlamaydi | Faqat **ulanadi** (NEHotspotConfiguration orqali, foydalanuvchi so'rovi bilan) | Ulanadi. Hotspot ham ocha oladi (NetworkOperatorTetheringManager) | Ulanadi. Dastur hotspot ocha olmaydi | Ulanadi. NetworkManager hotspot ocha oladi | Yo'q | `NEARBY_WIFI_DEVICES` (33+) yoki `ACCESS_FINE_LOCATION` | Ochiq | [32][33][34][35][37] |
| iPhone Personal Hotspot (qo'lda yoqiladi) | Tarmoqsiz (iPhone ishtirokida) | Raqam yo'q | Wi-Fi qamrovi | Ulanadi | ⚠️ Faqat foydalanuvchi qo'lda yoqadi | Ulanadi | Ulanadi | Ulanadi | Yo'q (lekin uyali tarif yoqilgan bo'ladi) | — | Yopiq | [34] |
| AWDL (Apple Wireless Direct Link) | Apple ⇄ Apple yaqin | MacBook↔iMac TCP ~0.7 Gbit/s atrofida, AP bilan teng [11]. Kanal almashtirilsa −13% [11] | Wi-Fi qamrovi | ❌ | ✅ faqat MPC / Network.framework `includePeerToPeer` orqali | ❌ | ✅ | ⚠️ OWL (tadqiqot, maxsus Wi-Fi karta) | Yo'q | Local Network ruxsati | Yopiq (reverse-engineered) | [11][30][93] |
| Multipeer Connectivity (iOS/macOS) | Apple ⇄ Apple | Eski anekdotlar: 16 Mbit/s va undan past (2013–2016). Yangi o'lchov topilmadi | Wi-Fi/BT qamrovi | ❌ | ✅ | ❌ | ✅ | ❌ | Yo'q | `NSBonjourServices` | Yopiq | [31] |
| BLE GATT (write-without-response) | Topish, kichik ma'lumot | iOS'da 400–600 kbit/s (anekdot, 2M PHY). Nazariy shift ~1.4 Mbit/s | Ditto: 100–200 m ichkarida (vendor) | ✅ | ✅ (fonda reklama cheklangan, tekshirilmagan) | ✅ | ✅ | ✅ BlueZ | Yo'q | Android: `BLUETOOTH_SCAN/CONNECT/ADVERTISE`. iOS: `NSBluetoothAlwaysUsageDescription` | Ochiq standart | [5][27][29] |
| BLE L2CAP CoC | Kichik va o'rta ma'lumot | iPhone 8 → nRF52 yo'nalishida ~380 kbit/s (anekdot) | BLE | ✅ Android 10+ | ✅ iOS 11+ (CBL2CAPChannel) | ⚠️ ? | ✅ | ✅ | Yo'q | BLE bilan bir xil | Ochiq | [28] |
| Bluetooth Classic RFCOMM / OBEX | Android ⇄ PC, eski qurilmalar | BR: ~720 kbit/s foydali, EDR: ~2.1 Mbit/s (glossariy) | ~10 m (tekshirilmagan) | ✅ | ❌ (faqat MFi aksessuar, tekshirilmagan) | ✅ | ⚠️ | ✅ | Yo'q | `BLUETOOTH_CONNECT` | Ochiq standart | [103] |
| Bluetooth PAN | Android ⇄ PC IP tarmog'i | Raqam yo'q | BT | ⚠️ (tethering sozlamasi) | ❌ | ✅ | ⚠️ | ✅ | Yo'q | Tizim sozlamasi | Ochiq | tekshirilmagan |
| NFC handover | Bir tegish bilan pairing | Data uchun emas (bootstrap) | Bir necha sm | ✅ (Android Beam Android 14'da olib tashlangan) | ⚠️ (P2P NFC yo'q, tekshirilmagan) | ⚠️ | ❌ | ⚠️ | Yo'q | NFC ruxsati | Ochiq standart | [84] |
| UWB | Faqat masofa va yo'nalish | Data emas | Bir necha m | ✅ Android 13+ (FiRa UCI) | ✅ Nearby Interaction (asosan MFi aksessuar) | ❌ | ❌ | ❌ | Yo'q | iOS: Nearby Interaction | FiRa (qisman ochiq) | [85] |
| USB: Android ADB / USB tethering (NCM/RNDIS) | Kabel, isolated Wi-Fi | Tethering: 25–60 Mbit/s, ba'zan 200–275 Mbit/s (anekdotlar) | Kabel | ✅ | — | ✅ | ✅ | ✅ | Yo'q | ADB uchun developer mode | Ochiq | [79] |
| USB: Android Open Accessory (AOA) | Kabel | Eski FTDI hujjatida "full speed" 12 Mbit/s. Zamonaviy o'lchov topilmadi | Kabel | ✅ | — | ✅ (libusb) | ✅ | ✅ | Yo'q | USB ruxsat dialogi | Ochiq | [80] |
| USB: iOS usbmuxd (peertalk usuli) | Kabel, iPhone ⇄ PC | O'lchov topilmadi | Kabel | — | ✅ (ilova TCP'da tinglaydi) | ✅ Apple Mobile Device Support kerak | ✅ | ✅ libimobiledevice | Yo'q | App Review xavfi bor (peertalk README'si) | Protokol yopiq, kutubxonalar ochiq | [81] |
| Animatsion QR (libcimbar) | Air-gap, kichik va o'rta fayl | 850 kbit/s ≈ 106 KB/s (vendor). Fayl ≤33 MB (siqilgandan keyin) | Kamera ko'radigan masofa | ✅ dekoder | ⚠️ (web enkoder bor, iOS dekoder ?) | ✅ enkoder (brauzer) | ✅ | ✅ | Yo'q | Kamera | MPL-2.0 | [73] |
| Animatsion QR (TXQR, LT-code) | Kichik ma'lumot | LT-code QR ~30 KB/s cho'qqi, odatda pastroq (blog). Decimen 128–186 KB/s (o'zi aytgan) | Kamera | ✅ | ✅ | ✅ | ✅ | ✅ | Yo'q | Kamera | MIT | [75][76] |
| Audio / ultratovush (ggwave) | Bir necha bayt (kod, URL) | 8–16 bayt/s | Xona | ✅ | ✅ | ✅ | ✅ | ✅ | Yo'q | Mikrofon | MIT | [74] |
| LoRa / Meshtastic | Matnli xabar, km masofa | LongFast ~1.07 kbit/s. ShortTurbo 21.9 kbit/s, ~1 km. VeryLongSlow 0.09 kbit/s, 40+ km (eskirgan preset) | km | ✅ (BLE orqali radio) | ✅ (BLE orqali radio) | ✅ | ✅ | ✅ | Yo'q | Alohida LoRa apparati kerak | GPL-3.0 | [77][78] |

### 1b. Internet usullari

| Usul | Vaziyat | Tezlik / narx | Android | iOS | Win | mac | Linux | Internet | Ochiq/yopiq | Manba |
|---|---|---|---|---|---|---|---|---|---|---|
| iroh: QUIC + hole punching + encrypted relay + pkarr DNS | Turli tarmoq, CGNAT | To'g'ridan-to'g'ri ulanish ~90% (uchinchi tomon blogi, **tekshirilmagan**). n0 public relay rate-limited va production uchun emas. Shared relay: 5 MB/s limit, 100 GB/oy (to'lovli tarif, sahifadan qayta tekshiring) | ✅ | ✅ | ✅ | ✅ | ✅ | Ha | Apache-2.0/MIT | [47][48][49] |
| libp2p: AutoNAT + Circuit Relay v2 + DCUtR | Turli tarmoq | 4.4 mln urinishda 70% ± 7.1%. TCP va QUIC deyarli bir xil. Muvaffaqiyatning 97.6% birinchi urinishda | ✅ | ✅ | ✅ | ✅ | ✅ | Ha | MIT | [40][41] |
| WebRTC (STUN/TURN/ICE) | Brauzer, turli tarmoq | Qo'ng'iroqlarning ~15–22% TURN'ga tushadi (eski yoki anekdot raqam). Relay qilinganlarning 78% TURN/TCP orqali. DataChannel bir mashinada ~30 MB/s (anekdot), RTT oshsa keskin pasayadi | ✅ | ✅ | ✅ | ✅ | ✅ | Ha | Ochiq standart | [53][54][63][64] |
| TURN xizmati (narx) | Relay | Cloudflare: $0.05/GB, oyiga 1000 GB tekin. Twilio: $0.40/GB (uchinchi tomon manbasi) | — | — | — | — | — | Ha | Yopiq xizmat | [51][52] |
| Tailscale / Headscale (DERP, peer relay) | Shaxsiy overlay tarmoq | To'g'ridan-to'g'ri ulanish ">90% typical" (ichki metrika). Ikkala tomon hard NAT bo'lsa → relay | ✅ | ✅ | ✅ | ✅ | ✅ | Ha (birinchi ulanish uchun) | BSD-3 (klient) | [43][44][45] |
| ZeroTier / Nebula / WireGuard | Overlay VPN | Raqam yo'q. WireGuard'ning o'zida NAT traversal yo'q | ✅ | ✅ | ✅ | ✅ | ✅ | Ha | turli | [102] |
| magic-wormhole transit relay | CLI, kod bilan | TCP relay. Raqam yo'q | ⚠️ (ilovalar orqali) | ⚠️ | ✅ | ✅ | ✅ | Ha | MIT | [57] |
| Shifrlangan cloud-handoff (yuklash + havola) | Qabul qiluvchi offline / boshqa vaqtda | Bulut tezligi. Saqlash va egress narxi sizning zimmangizda | ✅ | ✅ (background URLSession) | ✅ | ✅ | ✅ | Ha | — | [96] |
| Telegram bot / email | Kichik fayl, oddiy foydalanuvchi | Bot API: 50 MB yuklash / 20 MB olish. Local Bot API server bilan 2000 MB. Gmail: 25 MB | ✅ | ✅ | ✅ | ✅ | ✅ | Ha | Yopiq | [82][83] |

### 1c. Proprietar tizimlar (Vutric to'g'ridan-to'g'ri ishlata olmaydi, lekin qanday ishlashi foydali)

| Tizim | Topish → uzatish | Tezlik | Platforma | Eslatma | Manba |
|---|---|---|---|---|---|
| AirDrop | BLE → AWDL. Contacts-only rejimida Apple sertifikatlari ishlatiladi | AWDL ~0.7 Gbit/s laboratoriyada [11]. iOS 27'da "80% tezroq" degan va'da bor (Apple marketingi) | Apple | iOS 26.2: AirDrop kodlari, kontakt bo'lmagan odamga 30 kungacha ruxsat [22]. 2024 yilda Pekinda xesh loglari rainbow table bilan ochilgan [13]. PrivateDrop (USENIX Sec '21) PSI asosida tuzatish taklif qilgan [12] | [11][12][13][22][101] |
| Quick Share (Google, ex-Nearby Share) | BLE (+mDNS) → Wi-Fi LAN / Wi-Fi Direct / hotspot / BT / 5G P2P / Google serverlari orqali WebRTC | Pixel 8 ~25 MB/s, Wi-Fi Direct'da 20–55 MB/s (anekdotlar). Wi-Fi yo'q bo'lsa BT'ga tushadi: ~1 MB/s (HN anekdot) | Android, Windows (64-bit Win10+), Pixel 10 / Galaxy S26 ⇄ AirDrop | AirDrop bilan ishlash faqat "Everyone for 10 minutes" rejimida (Pixel 10: 2025-11-20, Galaxy S26: 2026-03-23) [20][21]. Interop qatlami Rust'da yozilgan (Google aytgan) [20] | [17][18][19][20][21][88][100] |
| Samsung Quick Share | Wi-Fi Direct | S20/Note20'da Wi-Fi Direct bilan bir xil tezlik [100] | Galaxy, ba'zi Windows PC (Intel BT/Wi-Fi drayverlari bilan) | Protocol Prying (2026): V4 OfflineFrame autentifikatsiyadan oldin, V5 D2D shifrlashni chetlab o'tish [14] | [14][100] |
| Huawei Share | BLE topish + NFC OneHop → Wi-Fi P2P | "80 MB/s gacha" (vendor SDK sahifasi). 2019: 30 MB/s (vendor) | Huawei / HarmonyOS | iPhone bilan ishlashi tasdiqlanmadi | [24] |
| Mutual Transmission Alliance (Xiaomi/OPPO/vivo, keyin Samsung ham qo'shilgani aytilgan) | Bluetooth pairing → Wi-Fi Direct | O'rtacha 20 MB/s (vendor) | Shu brendlar | Samsung a'zoligi 2021 yilgi xabarlarga asoslangan | [23] |
| Windows Nearby Sharing | BT + Wi-Fi | Raqam topilmadi | Windows | tekshirilmagan | — |
| Phone Link / Link to Windows (iPhone) | Microsoft akkaunti + BT/Wi-Fi (tekshirilmagan) | Raqam yo'q | Win10/11 + iOS 16+ | Fayl almashish 2024-12 dan bosqichma-bosqich chiqarilgan | [95] |
| Apple Continuity (Handoff, Universal Clipboard) | BLE + AWDL + iCloud (tekshirilmagan) | — | Apple | Uchinchi tomonga ochiq emas | — |
| Google Cross device SDK | BT + Wi-Fi + UWB | — | Faqat Android (2022 developer preview) | Hozirgi holati noma'lum | [94] |

---

## 2. Vaziyatlar bo'yicha eng yaxshi usullar (reyting)

### 2.1. Bir xil Wi-Fi / LAN
1. **To'g'ridan-to'g'ri QUIC/TCP LAN ichida** (iroh direct path). Tezlik Wi-Fi bilan cheklanadi. Bir taqqoslashda AirDrop 68–78 MB/s, LocalSend 24–42 MB/s ko'rsatgan, lekin manba ishonchliligi past [89].
2. **Topish zanjiri**, shu tartibda:
   - mDNS/DNS-SD (Vutric'da bor);
   - LocalSend'dagidek UDP multicast `224.0.0.0/24` ichida. Sababi: ba'zi Android qurilmalar boshqa guruhlarni rad etadi [55];
   - "legacy" unicast skan: subnetdagi har bir IP'ga HTTP so'rov yuboriladi [55];
   - **QR ticket** ichida barcha IP-hint'lar (link-local IPv6 ham).
3. **iOS tuzog'i.**
   - Birinchi local network amalida ogohlantirish chiqadi. Ilova fonda bo'lsa, amal jimgina rad etiladi [30].
   - Multicast va broadcast uchun Apple'dan `com.apple.developer.networking.multicast` entitlement'ini olish kerak [30].
   - Faqat unicast TCP va UDP bo'lsa, entitlement kerak emas [30].
4. **AP isolation va VLAN** LocalSend foydalanuvchilarining eng ko'p shikoyati [98]. Bu holatda 2.5-bo'limga o'ting.

### 2.2. Turli tarmoqlar (internet orqali)
1. **Hole punching + QUIC** (iroh). Muvaffaqiyat darajasi bo'yicha raqamlar:
   - libp2p o'lchovi: 70% ± 7.1% [40];
   - Tailscale: ">90%" [43];
   - port-restricted cone NAT: 82.9%, symmetric NAT: ~40% (kichik namuna) [41][42];
   - 2005 yilgi klassik ish: NAT'larning 82% UDP, 64% TCP hole punching'ni qo'llagan [39].
2. **O'z relay'ingiz** (`iroh-relay`, ACME TLS ichida). n0 public relay'lari faqat ishlab chiqish uchun, production uchun emas [47]. Relay trafigi uchun **rate limit** qo'ying [47].
3. **CGNAT** uyali tarmoqlarda deyarli hamma joyda: uyali AS'larning 90% dan ortig'i (2016) [46]. Ikkala tomon ham hard NAT bo'lsa, hole punching deyarli chiqmaydi va relay ishlatiladi [44].
4. Relay ham ishlamasa: shifrlangan **cloud-handoff** (yuklash + havola) va asinxron yetkazish.

### 2.3. Yaqin, lekin umumiy tarmoq yo'q (ikkalasida ham Wi-Fi radio bor)
1. **Android ⇄ hamma:** Android LocalOnlyHotspot. Telefon AP ochadi, boshqa qurilma QR'dagi SSID va parol bilan ulanadi.
   - iOS NEHotspotConfiguration bilan ulanadi. Foydalanuvchi so'rovni tasdiqlashi kerak [35].
   - iOS internetsiz tarmoqdan saqlangan tarmoqqa qaytib ketishi mumkin [36].
   - Flying Carpet aynan shu usulni ishlatadi, kalitlarni BLE orqali almashadi [34].
2. **Android ⇄ Android / Windows:** Wi-Fi Direct. Quick Share va MTA asosan shuni ishlatadi [23][100].
3. **iOS ⇄ iOS:** Network.framework `includePeerToPeer` (AWDL) yoki Wi-Fi Aware (iOS 26+, DeviceDiscoveryUI orqali pairing) [2][30].
4. **Android ⇄ Android:** Wi-Fi Aware (Android 8+). Pixel 7+ 8 tagacha NDP ushlab turadi, Pixel 4a esa 2 ta [9].
5. **iOS ⇄ Android Wi-Fi Aware:** nazariy jihatdan standart. Amalda:
   - iOS faqat v4 pairing'ni qabul qiladi (6 xonali PIN, ochiq datapath yo'q) [8];
   - Pixel 10 v4 pairing'ni qo'llamaydi [9];
   - Galaxy S25 bilan pairing xato 15 bilan tugagan [7].
   **Hozircha tayanib bo'lmaydi.**
6. **iPhone ⇄ PC:** iPhone Personal Hotspot qo'lda yoqiladi, PC unga ulanadi. Vutric testida PC shu yo'l bilan 172.20.10.x tarmog'ida bo'lgan.

### 2.4. Umuman tarmoq yo'q (Wi-Fi ishlatib bo'lmaydi yoki o'chirilgan)
1. **USB kabel:**
   - Android: ADB reverse, USB tethering yoki AOA;
   - iOS: usbmuxd orqali ilovadagi TCP portga ulanish (peertalk usuli).
   Bu eng ishonchli va eng tez offline yo'l. Tethering bo'yicha anekdotlar: 25–275 Mbit/s [79][81].
2. **Ethernet / Thunderbolt kabel** (kompyuter ⇄ kompyuter). Link-local IPv6 va mDNS kabel ustida ham ishlaydi (tekshirilmagan).
3. **BLE L2CAP / GATT:** faqat kichik fayllar uchun. iOS'da 400–600 kbit/s (anekdot) [27]. 10 MB taxminan 2–3 daqiqa oladi (hisob: 10 MB × 8 / 0.5 Mbit/s ≈ 160 s).
4. **Animatsion QR (libcimbar):** ≤33 MB, ~106 KB/s [73]. Radio taqiqlangan muhit uchun.
5. **Audio (ggwave):** 8–16 B/s [74]. Faqat pairing kodi yoki qisqa URL uchun.
6. **Sneakernet:** SD karta yoki flesh. Briar 1.4 shu kanalni rasmiy qo'shgan [69].

### 2.5. Mehmonxona yoki guest Wi-Fi (client isolation)
1. **Ikkalasi ham internetga chiqsa: relay orqali.** Bunda NAT hairpin yo'q va relay'ga TLS 443 orqali boriladi (iroh relay HTTPS). Tezlik relay va mehmonxona uplink'i bilan cheklanadi.
2. **Telefon hotspot'i:** LOHS yoki qo'lda yoqilgan Personal Hotspot. Mehmonxona Wi-Fi'sini chetlab o'tadi [34].
3. **Wi-Fi Aware:** infratuzilma Wi-Fi bilan bir vaqtda ishlay oladi, isolation unga ta'sir qilmaydi [1].
4. **USB kabel.**
5. **Captive portal** odatda local trafikni ham to'sadi. Foydalanuvchiga "bu tarmoq qurilmalarni bir-biridan ajratadi" deb aniq ko'rsatish kerak. Buni aniqlash: mDNS javobi yo'q, lekin internet bor.

### 2.6. Juda katta fayllar (10–100+ GB)
1. **Kabel (USB/Ethernet/Thunderbolt) yoki 5/6 GHz LAN direct.**
2. **Resume majburiy.** iroh-blobs BLAKE3 bilan tekshirilgan streaming va to'xtagan joydan davom ettirishni beradi [65]. croc ham resume qiladi [56].
3. **Relay'dan katta fayl o'tkazmaslik.** Relay narxi:
   - Cloudflare TURN bo'yicha 100 GB ≈ $5 (tekin 1000 GB'dan keyin) [51];
   - n0 shared relay 5 MB/s bilan cheklangan [48]. 100 GB 5 MB/s'da ≈ 5.6 soat oladi (hisob).
4. **iOS fon rejimi:**
   - Ekran o'chsa, ilova to'xtatiladi;
   - KDE Connect iOS ham "ilova ochiq tursin" deydi [67];
   - iOS 26 `BGContinuedProcessingTask` foydalanuvchi boshlagan ishni fonda davom ettira oladi [96]. Tarmoq uzatish uchun bu rasman kafolatlanganmi, **tekshirilmagan** [97].
   - Background `URLSession` faqat HTTP(S) server bilan ishlaydi, P2P uchun emas.

### 2.7. Kichik xabarlar (matn, havola, clipboard, pairing kodi)
1. **Mavjud kanal ustida** (LAN yoki relay). Kechikish ms darajasida.
2. **BLE GATT:** internet va Wi-Fi yo'q, yaqin masofa.
3. **Wi-Fi Aware discovery xabari:** ~255 bayt, ishonch yo'q (yo'qolishi yoki takrorlanishi mumkin) [10].
4. **QR yoki ggwave:** bir martalik kod.
5. **LoRa / Meshtastic:** km masofa, ~1 kbit/s. Lekin alohida radio kerak [77].
6. **DTN / opportunistik tarmoqlar** (store-carry-forward):
   - arxitektura RFC 4838 [91];
   - inson harakati va forward qilish bo'yicha klassik ish [90].
   Vutric uchun amaliy emas, faqat g'oya.

---

## 3. Ochiq manbali loyihalar: nima qiladi va biz nima o'rganamiz

| Loyiha | Topish | Transport / shifrlash | Internet | Yulduz, litsenziya | Bizga saboq |
|---|---|---|---|---|---|
| **LocalSend** | UDP multicast 224.0.0.167:53317 (224.0.0.0/24, chunki ba'zi Android boshqasini rad etadi) + HTTP unicast skan | HTTP/HTTPS (self-signed, fingerprint = sertifikat SHA-256), ixtiyoriy PIN | Yo'q | 99.4k, Apache-2.0 | Fallback zanjiri (multicast → unicast skan) va web "download API" eng ko'p yordam beradi. AP isolation eng katta og'riq [55][98] |
| **KDE Connect / GSConnect** | UDP broadcast (1716) + mDNS. Plasma 6'da Bluetooth ham bor (forum) | TCP 1714–1764 + TLS | Yo'q | KDE 4.2k (GPL), GSConnect 3.8k GPL-2.0 | iOS ilovasida fon rejimi yo'q, uzatish uchun ilova ochiq turishi kerak [66][67] |
| **Snapdrop** | Server bir xil public IP'dagi brauzerlarni bitta "xona"ga qo'yadi | WebRTC (STUN) | Faqat signalling | 19.7k, GPL-3.0 | IP bo'yicha guruhlash CGNAT va IPv6'da noto'g'ri ishlaydi [62] |
| **PairDrop** | LAN + 6 xonali pairing kodi/QR + 5 harfli public room | WebRTC, o'z TURN serveri | Ha (TURN) | 11.8k, GPL-3.0 | Doimiy pairing (shared secret) bitta tarmoqqa bog'liq emas. Biz QR-ticket bilan aynan shuni qilyapmiz [61] |
| **Warpinator** | Zeroconf (mDNS) + "group code" (8–32 belgi, sukut "Warpinator") | gRPC | Yo'q | 1.6k, GPL-3.0 | Sukut bo'yicha umumiy kod xavfli. README'ning o'zi uni almashtirishni maslahat beradi [68] |
| **croc** | Kod-ibora → 3 ta public relay'dan biri (xesh bo'yicha) | PAKE → E2E. Avval WireGuard/DERP ("Tailcat") bilan direct, bo'lmasa relay (9009–9013) | Ha | 41.0k, MIT | PAKE inson aytadigan kod bilan, relay'ni self-host qilish oson, resume [56] |
| **magic-wormhole** (+ Wormhole William, .rs) | Mailbox server + qisqa kod (PAKE) | Transit: direct TCP hint'lar bilan relay poygasi, NaCl secretbox. Dilation = uzoq yashovchi qayta ulanadigan kanal (0.19, 2025-05) | Ha | 23.1k MIT. William 1.3k MIT. .rs 1.1k EUPL-1.2 | "Avval direct, bir necha soniyadan keyin relay", qaysi yo'l birinchi bo'lsa o'sha g'olib [57][58][59] |
| **sendme / iroh-blobs** | iroh ticket (NodeId + manzillar) | QUIC + relay, BLAKE3 verified streaming, resume | Ha | sendme 1.3k, iroh 13.0k, iroh-blobs 181. Apache-2.0/MIT | Vutric uchun eng yaqin namuna. Katta fayl uchun iroh-blobs'ni to'g'ridan-to'g'ri ishlatish mumkin [65] |
| **Syncthing** | Local discovery (broadcast) + global discovery serveri | TLS. Relay pool: E2E, lekin sekin | Ha | 90.5k, MPL-2.0 | Relay operatori IP va trafik hajmini ko'radi. Buni foydalanuvchiga ochiq aytish kerak [60] |
| **OnionShare** | .onion manzil | Tor onion service | Ha (Tor) | 7.2k | Anonimlik uchun. Tezlik past (o'lchov topilmadi) |
| **Briar** | BT, Wi-Fi, Tor, flesh/SD (1.4), Mailbox (2023) | Bramble Transport Protocol | Ixtiyoriy | 723 (GitHub mirror) | Ko'p transport, bitta protokol. Yuqori kechikishli kanal (disk) ham to'liq transport deb qaraladi [69] |
| **Berty / Wesh** | Android Nearby + Multipeer Connectivity + BLE (cross-OS) | libp2p | Ixtiyoriy | berty 9.4k, weshnet 257 | Platformaning o'z P2P framework'larini libp2p drayveri sifatida o'rash. BLE Android'da beqaror bo'lgan [70] |
| **libp2p** (AutoNAT, Circuit Relay v2, DCUtR) | DHT / rendezvous | TCP/QUIC/WebRTC | Ha | rust-libp2p 5.6k MIT | Eng yaxshi ochiq NAT o'lchovi shu yerda: 70% ± 7.1% [40] |
| **WebTorrent** | Tracker / DHT | WebRTC | Ha | 31.5k, MIT | Ko'p manbali yuklab olish. 1:1 uchun ortiqcha |
| **Tailscale / Headscale** (+Taildrop) | Koordinatsiya serveri | WireGuard + DISCO, DERP, peer relay | Ha | 39.2k / 45.8k, BSD-3 | Peer relay g'oyasi: foydalanuvchining o'z kompyuteri relay bo'ladi [45] |
| **ZeroTier / Nebula** | Root/lighthouse | O'z overlay protokoli | Ha | 17.3k / 18.5k | Overlay VPN. Fayl uzatish ilovasi uchun og'ir |
| **NearDrop** (macOS Quick Share) | Mac QR ko'rsatadi, chunki macOS kerakli BLE reklamasini yubora olmaydi | Faqat Wi-Fi LAN | Yo'q | 6.4k, Unlicense | Quick Share protokoli PROTOCOL.md'da yozilgan. Wi-Fi Direct macOS'da yo'q [17] |
| **rquickshare / Packet** (Linux Quick Share) | mDNS (+BLE qisman) | Wi-Fi LAN (tekshirilmagan) | Yo'q | 3.7k / 1.2k, GPL-3.0 | Android bilan "native" ulanish faqat LAN orqali |
| **Flying Carpet** | BT orqali hotspot kalitlari yoki QR. "Shared network" rejimi ham bor | Hotspot + Noise (PBKDF2 600k) | Yo'q | 5.4k, GPL-3.0 | Vutric hotspot rejimi uchun eng yaqin namuna: Android 10+, Apple hotspot ocha olmaydi, ba'zi MIUI/HarmonyOS'da LOHS yo'q [34] |
| **OpenDrop + OWL** | AWDL (Linux) | AirDrop protokoli | Yo'q | 10.0k / 1.6k, GPL-3.0 | AWDL'ni Linux'da maxsus Wi-Fi karta bilan qilish mumkin. Mahsulot uchun emas [93] |
| **qrcp** | QR'da LAN URL | HTTP | Yo'q | 10.5k, MIT | QR + HTTP eng oddiy UX. Qabul qiluvchida ilova o'rnatilishi shart emas |
| **libcimbar / TXQR** | — | Kamera | Yo'q | 6.5k MPL-2.0 / 3.3k MIT | Air-gap uchun. 106 KB/s [73] |
| **ggwave** | — | Audio | Yo'q | 7.9k, MIT | Kod uzatish uchun [74] |
| **Meshtastic** | LoRa mesh | — | Yo'q | 8.6k, GPL-3.0 | km masofa, bayt hajmidagi xabarlar [77] |
| **Ditto** (yopiq SDK) | BLE + AWDL + LAN, Wi-Fi Aware rejalashtirilgan | Mesh sync | Ixtiyoriy | Yopiq | BLE "uyg'otish" uchun, bulk Wi-Fi orqali [5] |
| **Bridgefy** (yopiq SDK) | BLE mesh | libsignal (2022 dan) | Yo'q | Yopiq | 2021 va 2022 yillarda buzilgan (kuzatish, MITM). Mesh xavfsizligini qo'lda yasamaslik kerak [71][72] |
| **Send Anywhere, Blip, Feem, Xender, SHAREit, Zapya, Dukto** | 6 xonali kod / LAN / hotspot | Turli | Ba'zisi | Yopiq (Dukto GPL) | SHAREit Android'da Trend Micro 2021 zaifliklari: majburiy HTTPS yo'q, FileProvider muammosi [86]. Blip relay'i va o'lchami bo'yicha manbalar bir-biriga zid [87] |

**Umumiy saboqlar**
1. **Topish ≠ uzatish.** Hammasi kamida 2 ta topish yo'li va kamida 2 ta uzatish yo'lidan foydalanadi. Ishlab turgan tizimlarda arzon kanal (BLE yoki mDNS) qurilmani topadi, tez kanal (Wi-Fi) esa ma'lumotni olib o'tadi [23][24][100].
2. **Pre-auth parser eng katta xavf:**
   - QuickShell: 10 zaiflik, RCE zanjiri, majburiy Wi-Fi ulanish [15];
   - fix'ni chetlab o'tish CVE-2024-10668 [16];
   - Protocol Prying (2026): AirDrop'da 3 ta pre-auth DoS, Quick Share'da pre-auth dispatch va shifrlashni chetlab o'tish [14].
   Vutric'da autentifikatsiyadan oldin hech qanday murakkab format parse qilinmasligi kerak (protobuf, plist, rasm preview).
3. **Kontakt xeshlari oshkor bo'ladi.** AirDrop xeshlari rainbow table bilan ochilgan [12][13]. Kontaktlarni xesh qilib reklama qilmang. Tasodifiy, aylanadigan identifikator ishlating.
4. **"Everyone" rejimi vaqt bilan cheklangan bo'lishi kerak.** Apple ham 10 daqiqaga tushirgan [13][22].
5. **Sukut bo'yicha umumiy kod yoki ochiq ko'rinish xavfli** (Warpinator, NearDrop) [17][68].

---

## 4. Vutric uchun tavsiya

### 4.1. Hozir bor narsa va bo'shliqlar
Bor: LAN (iroh + mDNS), internet (direct + relay), QR-ticket pairing. Spike 0.2 ADR-0005 "internetsiz bir LAN"ni maqsad qilgan.

Bo'shliqlar:
- (a) multicast bloklangan LAN;
- (b) AP isolation;
- (c) umumiy tarmoq yo'q;
- (d) iOS fon rejimi;
- (e) relay'ga bog'liqlik (public n0 relay production uchun emas [47]).

### 4.2. Qo'shish tartibi (tavsiya)

| # | Usul | Nimani yopadi | Platforma | Murakkablik | Izoh |
|---|---|---|---|---|---|
| 1 | **LAN topishni mustahkamlash:** QR-ticket'ga barcha IP-hint'lar (link-local IPv6, 169.254, tethering subnetlari) + unicast "probe" + LocalSend'dagidek subnet skan (opt-in) | 2.1, 2.4 (kabel) | Hammasi | Past | iOS'da Local Network ruxsatini kontekst bilan so'rash. Rad etilganini `localNetworkDenied` orqali aniqlash [30] |
| 2 | **O'z iroh-relay'i** (ACME TLS, rate limit) + yo'l indikatori (direct / relay) | 2.2, 2.5 | Hammasi | Past | n0 public relay faqat zaxira. Relay operatori IP va hajmni ko'rishini ochiq yozish (Syncthing saboqi [60]) |
| 3 | **Hotspot rejimi:** Android LOHS + QR (SSID, parol, ticket) | 2.3, 2.5 | Android host. iOS, Win, Linux, mac ulanadi | O'rta | iOS'da NEHotspotConfiguration so'rovi chiqadi. iOS internetsiz tarmoqdan chiqib ketishi mumkin [36]. LOHS yo'q MIUI/HarmonyOS uchun fallback kerak [34]. Windows host: NetworkOperatorTetheringManager (Wi-Fi Direct bilan birga ishlamaydi) [37] |
| 4 | **BLE faqat topish va kalit almashish** (hotspot SSID/parol, ticket, uyg'otish) | 2.3 UX | Hammasi | O'rta | Bulk uchun emas: iOS 400–600 kbit/s (anekdot) [27]. Kontakt xeshlari emas, aylanadigan ID [13] |
| 5 | **USB kabel rejimi:** Android ADB reverse / USB tethering. iOS: usbmuxd → ilova TCP porti | 2.4, 2.5, 2.6 | Android, iOS ↔ desktop | O'rta | Windows'da iOS uchun Apple Mobile Device Support kerak (spike'da qo'lda o'rnatilgan). App Review xavfi bor [81] |
| 6 | **Wi-Fi Aware, platforma ichida:** Android ⇄ Android (NAN datapath), iOS ⇄ iOS (DeviceDiscoveryUI pairing) | 2.3, 2.5 | Android 8+, iOS 26+ (iPhone 12+) | Yuqori | iOS'da Rust iroh soketini NAN interfeysiga bog'lab bo'lmasligi mumkin, Network.framework adapteri kerak (tekshirilmagan). Entitlement: `com.apple.developer.wifi-aware` [1][2] |
| 7 | **Wi-Fi Direct:** Android ⇄ Android, Android ⇄ Windows | 2.3 | Android, Windows | Yuqori | Ishlab chiqarilgan qurilmalarda tezlik ~20 Mbit/s dan ~55 MB/s gacha tarqoq [25][99] |
| 8 | **Shifrlangan cloud-handoff** (opt-in): mijoz tomonda shifrlab S3-mos bucket'ga yuklash, kalit URL fragmentida | 2.2 (qabul qiluvchi offline) | Hammasi | O'rta | iOS'da background URLSession bilan ishlaydi. Saqlash muddati va hajm limiti kerak |
| 9 | **Animatsion QR (libcimbar)** | 2.4 air-gap | Desktop enkoder, Android dekoder | O'rta | ≤33 MB, ~106 KB/s [73] |
| 10 | **Wi-Fi Aware cross-platform (iOS ⇄ Android)** | 2.3 iPhone ⇄ Android | iOS 26+, Android (v4 pairing) | Yuqori | Bugun ishlamaydi [7][9]. Har chorakda qayta tekshiring |
| — | Qilmaslik | | | | Bluetooth Classic (iOS'da yo'q), NFC (iOS'da P2P yo'q), LoRa (alohida apparat), ultratovushli bulk (8–16 B/s) |

**Xavfsizlik shartlari (har bir yangi transport uchun):**
- autentifikatsiyadan oldin faqat qat'iy chegaralangan, oddiy freymlar [14][15];
- barcha freym turlari bir xil shifrlash va autentifikatsiyadan o'tadi (Samsung V5 saboqi [14]);
- protokol-aware fuzzing (AIRFUZZ uslubida [14]);
- 6 xonali tekshiruv kodi TLS exporter'dan olinadi (ADR-0005'da bor). Shu yo'l saqlansin.

### 4.3. iOS'da nima mumkin emas va nega

| Imkonsiz yoki cheklangan | Sabab | Manba |
|---|---|---|
| Wi-Fi Direct | iOS'da ommaviy API yo'q. Apple o'z AWDL'idan foydalanadi | [17] |
| Dasturiy hotspot ochish | Apple qurilmalari hotspot'ni dastur orqali ocha olmaydi (faqat foydalanuvchi Personal Hotspot'ni qo'lda yoqadi) | [34] |
| AWDL'ga to'g'ridan-to'g'ri soket | `awdl0`'ga bog'langan oddiy TCP/UDP serverlarning paketlarini Apple tashlab yuboradi. Faqat MPC yoki Network.framework `includePeerToPeer` orqali, va faqat Apple ⇄ Apple | [11][30] |
| Android bilan Wi-Fi Aware (hozircha) | iOS faqat v4 pairing (PIN) talab qiladi. Android firmware'lari buni hali qo'llamaydi (Pixel 10 ham) | [7][8][9] |
| Wi-Fi Aware'ni macOS'da ishlatish | Framework faqat iOS/iPadOS 26 da bor | [1][88] |
| Bluetooth Classic (RFCOMM/OBEX) | Faqat MFi aksessuarlar (tekshirilmagan) | [103] |
| NFC P2P (Android Beam uslubida) | iOS ochmaydi (tekshirilmagan) | — |
| Local network'ga fonda birinchi kirish | Ruxsat aniqlanmagan holatda fonda amal ogohlantirishsiz rad etiladi | [30] |
| Multicast/broadcast entitlement'siz | `com.apple.developer.networking.multicast` Apple tasdig'i bilan beriladi | [30] |
| Uzoq P2P uzatishni fonda davom ettirish | Ilova to'xtatiladi. `BGContinuedProcessingTask` (iOS 26) yordam berishi mumkin, tekshirilmagan | [96][97] |
| AirDrop yoki Quick Share'ga o'z ilovamizni ulash | Protokollar yopiq. Uchinchi tomon faqat share sheet orqali kiradi | [14] |

**EU DMA nima o'zgartirdi:**
- 2025-03-19 qarorlari Apple'dan quyidagilarni talab qildi [4][5]:
  - Wi-Fi Aware 4.0 ni "iOS 19" da (amalda iOS 26) berish;
  - Wi-Fi Aware 5.0 ni spec chiqqandan keyin 9 oy ichida berish.
- Apple framework'ni butun dunyoda ochdi [3].
- iOS 26.3 da EU uchun aksessuarlarga proximity pairing qo'shildi [6].
- "AWDL majburan o'chiriladi" degan da'vo (Ditto [5]) bahsli. HN'da bunga qarshi fikrlar bor [88]. **Tekshirilmagan.**

### 4.4. Foydalanuvchiga ko'rsatiladigan halol raqamlar

Tavsiya: **va'da qilingan tezlikni emas, jonli o'lchangan tezlikni ko'rsating.** Yo'l turini ham yozing: "To'g'ridan-to'g'ri · LAN", "To'g'ridan-to'g'ri · internet", "Relay orqali", "Hotspot", "Kabel". Statik matnda faqat manbali diapazonlar bo'lsin:

| Yo'l | UI'da yoziladigan matn | Asos |
|---|---|---|
| LAN direct | "Wi-Fi tezligingizda. Odatda o'nlab MB/s" | [89] (ishonchliligi past), [11] |
| Hotspot / Wi-Fi Direct | "Telefonga qarab ~2 dan ~50 MB/s gacha" | [25] (16–20 Mbit/s), [99][100] |
| Internet direct | "Ikkala internet kanalining sekinrog'i darajasida" | — (fizik cheklov) |
| Relay | "Relay orqali, sekinroq bo'lishi mumkin" + relay limiti (masalan, n0 shared 5 MB/s) | [47][48] |
| BLE | "Faqat kichik fayllar: ~50 KB/s" (400 kbit/s ≈ 50 KB/s) | [27] |
| Animatsion QR | "~100 KB/s, 33 MB gacha" | [73] |
| Internet orqali ulanish ehtimoli | "Ko'pincha to'g'ridan-to'g'ri ulanadi. Uyali internetda relay ehtimoli yuqori" | [40][43][46] |

Ko'rsatmaslik kerak: "AirDrop'dan tezroq", "har doim P2P", "100% offline" kabi da'volar. Bular manba bilan tasdiqlanmaydi.

### 4.5. Keyingi o'lchov ishlari
1. **Real qurilmalarda o'lchash:** iPhone 16 Pro ⇄ Windows (LAN, iPhone hotspot) va Android ⇄ Windows (LAN, LOHS). Vutric spike 0.2'da real Wi-Fi telefon testi hali kutilmoqda.
2. **iroh direct vs relay ulushi:** o'z telemetriyangiz (opt-in, anonim). Hozircha iroh bo'yicha ishonchli ochiq raqam yo'q [49][50].
3. **Hotspot tezligi:** LOHS 2.4 GHz va 5 GHz farqi. Sukut bo'yicha band tekshirilmagan [33].

---

## 5. Manbalar

> Har bir havola 2026-11-18 holatiga ko'ra. Sessiya davomida faqat qidiruv natijasidan (sahifa ochilmasdan) olingan raqamlar matnda "tekshirilmagan" yoki "anekdot" deb belgilangan.

**Apple, iOS, DMA**
1. Apple, Wi-Fi Aware framework: https://developer.apple.com/documentation/wifiaware
2. Apple, Building peer-to-peer apps (Wi-Fi Aware): https://developer.apple.com/documentation/wifiaware/building-peer-to-peer-apps
3. MacRumors, iOS 26 adds two new Wi-Fi features: https://www.macrumors.com/2025/06/21/ios-26-adding-two-new-wi-fi-features/
4. European Commission, DMA interoperability decisions (2025-03-19): https://digital-markets-act.ec.europa.eu/commission-provides-guidance-under-digital-markets-act-facilitate-development-innovative-products-2025-03-19_en
5. Ditto, Cross-platform P2P Wi-Fi: how the EU killed AWDL (vendor): https://www.ditto.com/blog/cross-platform-p2p-wi-fi-how-the-eu-killed-awdl
6. Heise, iOS 26.3 DMA interop: https://heise.de/-11118775 · Engadget: https://www.engadget.com/mobile/apples-ios-263-will-introduce-proximity-pairing-to-third-party-devices-in-the-eu-133037696.html
7. Apple Developer Forums, Wi-Fi Aware can't pair with Android: https://developer.apple.com/forums/thread/801280
8. Espressif, Wi-Fi Aware ESP to iPhone: https://developer.espressif.com/blog/2026/08/wifi-aware-esp-to-iphone/
9. grjte, Cross-platform Wi-Fi Aware (blog): https://hackmd.io/@grjte/cross-platform-wifi-aware

**Android, Wi-Fi P2P**

10. Android, Wi-Fi Aware: https://developer.android.com/develop/connectivity/wifi/wifi-aware

**AirDrop, AWDL, Quick Share va xavfsizlik**

11. Stute, Kreitschmann, Hollick, "One Billion Apples' Secret Sauce" (MobiCom'18): https://arxiv.org/abs/1808.03156
12. Heinrich va boshq., "PrivateDrop" (USENIX Security'21): https://www.usenix.org/conference/usenixsecurity21/presentation/heinrich (sahifa sessiyada ochilmagan, bibliografik ma'lumot)
13. TechTarget, China claims it cracked AirDrop (2024): https://www.techtarget.com/searchsecurity/news/366565895/China-claims-it-cracked-Apples-AirDrop-can-track-senders
14. Ale Ebrahim, Tippenhauer, "Protocol Prying" (2026): https://arxiv.org/abs/2606.26967
15. SafeBreach, QuickShell: https://github.com/SafeBreach-Labs/QuickShell
16. The Hacker News, Google patches Quick Share (2025): https://thehackernews.com/2025/04/google-patches-quick-share.html
17. NearDrop README: https://github.com/grishka/NearDrop
18. Android Authority, Quick Share for Windows Wi-Fi Direct: https://www.androidauthority.com/quick-share-for-windows-wifi-direct-3494451/
19. Google, Quick Share with Windows: https://support.google.com/android/answer/13801258
20. Android Authority, Quick Share works with AirDrop: https://www.androidauthority.com/quick-share-works-with-airdrop-3618023/
21. Samsung, AirDrop support on Galaxy S26: https://news.samsung.com/us/samsung-brings-airdrop-support-to-quick-share-with-galaxy-s26-series/
22. MacRumors, iOS 26.2 AirDrop codes: https://www.macrumors.com/2025/11/17/ios-26-2-airdrop-codes/
23. PRNewswire, vivo/OPPO/Xiaomi file transfer alliance (vendor): https://prnewswire.com/news-releases/vivo-oppo-and-xiaomi-partner-to-bring-new-wireless-file-transfer-system-to-global-consumers-300980373.html
24. Huawei Share Engine (vendor): https://developer.huawei.com/consumer/en/share-kit
25. arXiv 2604.07889, Wi-Fi Direct multi-group on stock Android: https://arxiv.org/abs/2604.07889
26. Android Developers (Medium), Nearby Connections: https://medium.com/androiddevelopers/two-way-communication-without-internet-nearby-connections-b118530cb84d

**Bluetooth va iOS tarmoq qoidalari**

27. Apple Developer Forums, Core Bluetooth throughput: https://developer.apple.com/forums/thread/770717
28. Nordic DevZone, iOS BLE throughput: https://devzone.nordicsemi.com/f/nordic-q-a/40597/maximizing-nrf52-ios-ble-throughput-in-both-directions
29. Memfault, BLE throughput primer: https://interrupt.memfault.com/blog/ble-throughput-primer
30. Apple TN3179, Local network privacy: https://developer.apple.com/documentation/technotes/tn3179-understanding-local-network-privacy
31. Apple Developer Forums, MPC slow: https://developer.apple.com/forums/thread/718235

**Hotspot va Windows**

32. Android, LocalOnlyHotspot: https://developer.android.com/develop/connectivity/wifi/localonlyhotspot
33. Gist (LOHS custom config, unofficial): https://gist.github.com/mikedawson/f6f5c69d23ce7228dcaf2e03f933777b
34. Flying Carpet README: https://github.com/spieglt/FlyingCarpet
35. Apple, NEHotspotConfigurationManager.apply: https://developer.apple.com/documentation/networkextension/nehotspotconfigurationmanager/apply(_:completionhandler:)
36. Apple Developer Forums, iOS leaves no-internet Wi-Fi: https://developer.apple.com/forums/thread/848895
37. Microsoft, NetworkOperatorTetheringManager: https://learn.microsoft.com/en-us/uwp/api/Windows.Networking.NetworkOperators.NetworkOperatorTetheringManager
38. Microsoft sample, MobileHotspot: https://github.com/microsoft/Windows-universal-samples/blob/main/Samples/MobileHotspot

**NAT traversal, relay**

39. Ford, Srisuresh, Kegel (USENIX ATC'05): https://bford.info/pub/net/p2pnat/
40. "Challenging Tribal Knowledge", DCUtR measurement: https://arxiv.org/abs/2510.27500
41. IMC'26 version: https://arxiv.org/abs/2604.12484
42. ProbeLab, Can libp2p punch through NATs?: https://probelab.io/blog/can-libp2p-punch-through-nats/
43. Tailscale, NAT traversal improvements pt.1: https://tailscale.com/blog/nat-traversal-improvements-pt-1
44. Tailscale, Device connectivity: https://tailscale.com/kb/1411/device-connectivity
45. Tailscale, DERP servers: https://tailscale.com/docs/reference/derp-servers
46. Richter va boshq., Carrier-Grade NAT (IMC'16): https://arxiv.org/abs/1605.05606
47. iroh docs, relays and rate limiting: https://docs.iroh.computer/relays/rate-limiting · https://docs.iroh.computer/concepts/relays
48. iroh, shared relays: https://docs.iroh.computer/iroh-services/relays/shared
49. Pinggy, "Iroh 1.0" (third-party, ~90% direct, tekshirilmagan): https://pinggy.io/blog/iroh_1_0_dial_keys_not_ips/
50. iroh blog, comparing iroh and libp2p: https://www.iroh.computer/blog/comparing-iroh-and-libp2p
51. Cloudflare Realtime TURN FAQ/pricing: https://developers.cloudflare.com/realtime/turn/faq/
52. Fora Soft, TURN bandwidth calculator (Twilio narxi, third-party): https://www.forasoft.com/learn/video-streaming/articles-streaming/turn-bandwidth-calculator
53. Philipp Hancke, What kind of TURN server is being used?: https://medium.com/@fippo/what-kind-of-turn-server-is-being-used-d67dbfc2ff5d
54. webrtcHacks, usage stats (callstats 2016): https://webrtchacks.com/usage-stats/

**Ochiq manbali loyihalar**

55. LocalSend protocol: https://github.com/localsend/protocol
56. croc README: https://github.com/schollz/croc
57. magic-wormhole Transit: https://magic-wormhole.readthedocs.io/en/latest/transit.html
58. magic-wormhole Dilation: https://magic-wormhole.readthedocs.io/en/latest/dilation-protocol.html · 0.19.0: https://lists.tahoe-lafs.org/pipermail/tahoe-dev/2025-May/010128.html
59. NLnet, Winden Dilation: https://nlnet.nl/project/Winden-MWH-Dilation/
60. Syncthing relaying: https://docs.syncthing.net/users/relaying.html
61. PairDrop README: https://github.com/schlagmichdoch/PairDrop
62. How Snapdrop works: https://snap-drop.net/how-it-works
63. ShareDrop issue #154 (WebRTC ~30 MB/s): https://github.com/szimek/sharedrop/issues/154
64. discuss-webrtc, throughput vs RTT: https://groups.google.com/g/discuss-webrtc/c/0synE_0zeCQ
65. sendme README: https://github.com/n0-computer/sendme
66. KDE Connect README: https://github.com/KDE/kdeconnect-kde · ports (forum): https://forum.manjaro.org/t/plasma-firewall-blocks-kde-connect-from-connecting-and-the-device-being-shown-in-the-mobile-app/83383
67. OMG! Ubuntu, KDE Connect iPhone app: https://www.omgubuntu.co.uk/2022/05/kde-connect-iphone-app-available
68. Warpinator README: https://github.com/linuxmint/warpinator
69. Briar, How it works: https://briarproject.org/how-it-works/ · 1.4: https://briarproject.org/news/2021-briar-1.4-released/ · Mailbox: https://briarproject.org/news/2023-briar-mailbox-released/
70. Berty/Wesh protocol: https://berty.tech/protocol
71. Albrecht va boshq., Breaking Bridgefy (CT-RSA'21): https://ia.cr/2021/214
72. Albrecht, Eikenberg, Paterson, Breaking Bridgefy, again (USENIX'22): https://www.usenix.org/conference/usenixsecurity22/presentation/albrecht

**Optik, audio, LoRa, USB, bulut**

73. libcimbar: https://github.com/sz3/libcimbar
74. ggwave: https://github.com/ggerganov/ggwave
75. TXQR: https://github.com/divan/txqr
76. Decimen (o'zi aytgan tezlik, third-party): https://zeli.app/story/49157792
77. Meshtastic LoRa config: https://meshtastic.org/docs/configuration/radio/lora
78. D-Central, Meshtastic presets (third-party): https://d-central.tech/meshtastic-modem-presets/
79. GL-iNet forum (USB tethering anekdotlari): https://forum.gl-inet.com/t/slow-usb-tethering-on-gl-sft1200-with-android-11-phone/66477 · https://forums.androidcentral.com/threads/maximum-tethering-speed-via-usb.828806
80. AOSP, Android Open Accessory: https://source.android.com/docs/core/interaction/accessories/aoa · FTDI WP_001 (2012)
81. peertalk: https://github.com/rsms/peertalk
82. Telegram limits: https://limits.tginfo.me · Bot FAQ: https://core.telegram.org/bots/faq
83. Gmail attachment limit (third-party): https://www.ionos.com/digitalguide/e-mail/technical-matters/gmail-attachment-size-limit/
84. Wikipedia, Android Beam: https://en.wikipedia.org/wiki/Android_Beam
85. Apple, Nearby Interaction UWB Interoperability Spec R3: https://developer.apple.com/download/files/Nearby-Interaction-with-UWB-Interoperability-Specification-R3.pdf
86. SecurityWeek, SHAREit vulnerabilities (Trend Micro): https://www.securityweek.com/several-vulnerabilities-found-popular-file-sharing-app-shareit
87. Blip: https://blip.net/

**Forumlar va ishonchliligi past manbalar**

88. Hacker News, Pixel 10 AirDrop thread: https://news.ycombinator.com/item?id=45994854
89. dev.to, LocalSend vs AirDrop vs Snapdrop benchmark (ishonchliligi past): https://dev.to/michael_sun_18a5c4c96768d/localsend-vs-airdrop-vs-snapdrop-vs-pixeldrop-100-files-cross-platform-transfer-real-world-speed-2fc4

**Akademik: DTN va Linux NAN**

90. Chaintreau va boshq., Impact of Human Mobility on Opportunistic Forwarding (IEEE TMC 2007), DOI 10.1109/TMC.2007.1060: https://www.cl.cam.ac.uk/~jac22/out/pocket.pdf
91. RFC 4838, DTN Architecture: https://www.rfc-editor.org/rfc/rfc4838
92. Linux nl80211 spec (NAN buyruqlari): https://docs.kernel.org/7.3/netlink/specs/nl80211.html

**Qo'shimcha**

93. OpenDrop: https://github.com/seemoo-lab/opendrop · OWL: https://github.com/seemoo-lab/owl
94. Android Developers Blog, Cross device SDK (2022): https://android-developers.googleblog.com/2022/07/announcing-cross-device-SDK-Developer-Preview-for-Android.html
95. nextpit, Phone Link iPhone: https://nextpit.com/pair-your-iphone-windows-phone-link-how-to
96. Apple, BGContinuedProcessingTask: https://developer.apple.com/documentation/backgroundtasks/bgcontinuedprocessingtask
97. Apple Developer Forums 809661: https://developer.apple.com/forums/thread/809661
98. TheWindowsClub, LocalSend not finding devices (AP isolation): https://www.thewindowsclub.com/localsend-not-working-or-finding-device-on-windows-pc
99. Lemmy (Samsung Wi-Fi Direct 54.7 MB/s anekdoti, aniq izoh tekshirilmagan): https://lemm.ee/comment/13585804
100. PhoneArena, Galaxy S20 Quick Share speeds: https://www.phonearena.com/news/samsung-galaxy-s20-note-20-ultra-quick-share-large-file-video-transfer_id122661
101. BornCity, iOS 27 "80% faster AirDrop": https://borncity.com/news/ios-27-apple-verspricht-80-schnelleres-airdrop-und-30-flottere-apps/
102. GitHub REST API `GET /repos/{owner}/{repo}` (yulduz va litsenziya, 2026-11-18)
103. RF Essentials, Bluetooth Classic glossary: https://rfessentials.com/resources/rf-glossary/bluetooth-classic/
