// UI copy behind stable keys. Uzbek (Latin) is the language people see; English is the fallback when a key has no
// Uzbek line yet. Keys that come from Rust (route.*, privacy.*, find.*, lan.unreachable.*) are W3's stable names
// (ADR-0052, 0059, 0060, 0061); the phone-only keys live under the screen they belong to.

import 'dart:ui' show PlatformDispatcher;

const Map<String, String> _uz = {
  // Route picker (ADR-0060).
  'route.title': 'Yoʻl',
  'route.picker.auto': 'Avtomatik',
  'route.picker.auto.hint': 'Shu tarmoq, keyin internet, kerak boʻlsa hotspot',
  'route.picker.lan_only': 'Faqat shu tarmoq',
  'route.picker.lan_only.hint': 'Hech narsa internetga chiqmaydi',
  'route.picker.internet': 'Internet orqali',
  'route.picker.internet.hint': 'Qurilmalar boshqa-boshqa tarmoqda',
  'route.picker.offline': 'Tarmoqsiz',
  'route.picker.offline.hint': 'Router ham, internet ham yoʻq: telefon hotspoti',
  'route.picker.bluetooth': 'Bluetooth',
  'route.picker.bluetooth.hint': 'Tez orada',
  'route.none.not_built': 'Bu yoʻl hali tayyor emas',
  'route.none.unavailable': 'Bu yoʻl hozir ishlamaydi: tarmoqni tekshiring yoki boshqa yoʻlni tanlang',
  'route.restart': 'Yoʻl oʻzgardi, qayta ulanmoqda…',
  // Route badge on an open session.
  'route.badge.direct_lan': 'Toʻgʻridan-toʻgʻri · LAN',
  'route.badge.direct_internet': 'Toʻgʻridan-toʻgʻri · internet',
  'route.badge.relay': 'Relay orqali',
  'route.badge.hotspot': 'Hotspot',
  'route.badge.cable': 'Kabel',
  'route.badge.connecting': 'Yoʻl tanlanmoqda…',
  // Privacy line (ADR-0061).
  'privacy.encrypted': 'Fayllar shifrlangan, faqat siz va qabul qiluvchi koʻradi',
  'privacy.nothing_leaves': 'Hech narsa internetga chiqmaydi',
  // Typed address or short code (ADR-0059).
  'find.title': 'Kod yoki manzil',
  'find.subtitle': 'QR skanerlanmasa: qurilmadagi kod yoki IP manzil',
  'find.hint': 'K7QD-2M9X yoki 192.168.1.24',
  'find.kind.link': 'Ulash havolasi',
  'find.kind.address': 'IP manzil',
  'find.kind.code': 'Qisqa kod',
  'find.scan': 'Butun tarmoqni ham qidirish',
  'find.go': 'Topish',
  'find.silent': 'Hech kim javob bermadi: kod yoki manzilni, ikkala qurilma bir Wi-Fi’da ekanini tekshiring',
  'find.denied': 'Telefon mahalliy tarmoqqa chiqishni taqiqladi: Sozlamalarda ruxsat bering',
  'find.unreadable': 'Bu kod ham, manzil ham emas',
  'find.no_beacon': 'Tarmoqda qidirib boʻlmaydi: Wi-Fi’ga ulaning',
  'find.my_code': 'Kodim',
  // Six-digit pairing (ADR-0050).
  'pair.title': 'Raqamlar bir xilmi?',
  'pair.body': 'Ikkala ekranda shu olti raqam chiqishi kerak. Bir xil boʻlsa, qurilma ishonchli boʻladi.',
  'pair.yes': 'Bir xil',
  'pair.no': 'Farq qiladi',
  'pair.waiting': 'Ikkinchi qurilma javobi kutilmoqda…',
  'pair.refused': 'Juftlash bekor qilindi',
  // Who may connect (ADR-0050).
  'visibility.title': 'Kim yubora oladi',
  'visibility.trusted_only': 'Faqat ishonchli qurilmalar',
  'visibility.everyone': 'Hamma, 10 daqiqa',
  'visibility.off': 'Toʻxtatilgan',
  'visibility.everyone.left': 'yana {m} daqiqa',
  // Devices (T3).
  'devices.title': 'Qurilmalar',
  'devices.empty': 'Hali juftlangan qurilma yoʻq',
  'devices.method.ticket': 'QR orqali',
  'devices.method.code': 'Raqam bilan',
  'devices.method.pinned': 'Boshqa ilova',
  'devices.seen': 'oxirgi marta {d}',
  'devices.rules': 'Shu qurilma uchun',
  'devices.rules.hint': 'Boʻsh qolsa, umumiy sozlama ishlaydi',
  'devices.rules.general': 'Umumiy',
  'devices.forget': 'Unutish',
  'devices.forget.confirm': 'Qurilma unutiladi. Keyingi safar qayta juftlash kerak.',
  // Hotspot (R3, ADR-0058).
  'hotspot.join.title': '“{ssid}” Wi-Fi’ga ulanamizmi?',
  'hotspot.join.body': 'Bu QR telefon hotspotini olib keldi. Ulangach, juftlash oʻzi davom etadi.',
  'hotspot.join.go': 'Ulanish',
  'hotspot.join.wait': 'Wi-Fi’ga ulanmoqda…',
  'hotspot.join.failed': 'Wi-Fi’ga ulanib boʻlmadi',
  'hotspot.join.manual': 'Telefon sozlamalarida “{ssid}” tarmogʻiga qoʻlda ulaning, keyin qayta urinib koʻring.',
  'hotspot.qr.vutric': 'Vutric bilan',
  'hotspot.qr.camera': 'Kamera bilan',
  // Companion (A12).
  'companion.title': 'Kompyuter bilan bogʻlash',
  'companion.notifications': 'Bildirishnomalarni kompyuterga koʻrsatish',
  'companion.notifications.hint': 'Oʻchiq. Yoqilsa, Android ruxsat soʻraydi; faqat tanlangan ilovalar',
  'companion.clipboard': 'Buferni yuborish',
  'companion.clipboard.hint': 'Oʻchiq. Yoqilsa, “Buferni yuborish” tugmasi chiqadi',
  'companion.clipboard.send': 'Buferni yuborish',
  'companion.clipboard.empty': 'Bufer boʻsh',
  'companion.clipboard.too_long': 'Buferdagi matn juda uzun (64 KB dan koʻp)',
  'companion.clipboard.sent': 'Bufer yuborildi',
  'companion.clipboard.came': '{peer} bufer matnini yubordi',
  'companion.clipboard.copy': 'Nusxa olish',
  'companion.clipboard.close': 'Yopish',
  'companion.clipboard.copied': 'Buferga nusxalandi',
  'companion.untrusted': 'Bu faqat juftlangan qurilma bilan ishlaydi',
  'companion.waiting': 'Juftlangan kompyuter ulanganda yuboriladi',
  'companion.apps': 'Qaysi ilovalar',
  'companion.apps.none': 'Hech biri tanlanmagan: hech narsa koʻrsatilmaydi',
  'companion.apps.some': '{n} ta ilova tanlangan',
  'companion.apps.search': 'Ilovani qidirish',
  'companion.apps.empty': 'Ilova topilmadi',
  'companion.apps.hint': 'Faqat belgilangan ilovalarning bildirishnomalari kompyuterga boradi',
  // Foreign apps nearby (A13, C10).
  'foreign.title': 'Yaqindagi boshqa ilovalar',
  'foreign.page': 'Boshqa ilovalar',
  'foreign.localsend': 'LocalSend',
  'foreign.kdeconnect': 'KDE Connect',
  'foreign.quickshare': 'Quick Share',
  'foreign.wormhole': 'magic-wormhole',
  'foreign.via': '{app} orqali',
  'foreign.none': 'Hech narsa topilmadi',
  'foreign.hint': 'Bosing: LocalSend va KDE Connect qurilmalariga yuborish',
  'compat.looking': 'LocalSend va KDE Connect qurilmalari qidirilmoqda…',
  'compat.hint': 'Ular sizga yuborsa, har safar soʻraladi. Juftlanmagan qurilmalar ishonchli boʻlmaydi.',
  'compat.unavailable': 'Moslik moduli ishga tushmadi: faqat qurilmalar roʻyxati koʻrinadi',
  'compat.off': 'Moslik moduli toʻxtagan',
  'compat.bad_path': 'Kelgan fayl notoʻgʻri joyga yozildi va olinmadi',
  'compat.kde_pair_first': 'avval KDE Connect ilovasidan juftlang',
  'compat.kde_unpaired': 'KDE Connect qurilmasi bilan avval uning ilovasidan juftlang',
  'compat.offer.title': '{peer} fayl yubormoqchi',
  'compat.offer.body': '{n} ta fayl, {size} · {app}',
  'compat.pair.title': '{peer} juftlashmoqchi',
  'compat.pair.body': 'Ikkala ekranda shu kod chiqishi kerak: {code}',
  'compat.pair.yes': 'Juftlash',
  'compat.accept': 'Qabul qilish',
  'compat.refuse': 'Rad etish',
  'compat.sent': '{peer}ga yuborildi',
  'compat.received': '{n} ta fayl qabul qilindi',
  // magic-wormhole (C7, ADR-0067).
  'wormhole.title': 'Kod bilan (magic-wormhole)',
  'wormhole.internet': 'Internet orqali ochiq serverlar bilan ishlaydi; fayl kod bilan shifrlanadi',
  'wormhole.send': 'Kod bilan yuborish',
  'wormhole.send.sub': 'Bitta fayl; kodni qabul qiluvchiga ayting',
  'wormhole.receive': 'Kod bilan qabul qilish',
  'wormhole.receive.sub': 'Yuboruvchi aytgan kodni kiriting',
  'wormhole.code.title': 'Kod',
  'wormhole.code.body': 'Shu kodni qabul qiluvchiga ayting. Kod bir martalik, uni boshqa hech kimga bermang.',
  'wormhole.code.hint': '7-guitarist-revenge',
  'wormhole.sent': 'Kod bilan yuborildi',
  'wormhole.lan_only': '“Faqat shu tarmoq” tanlangan: kod bilan yuborish internet talab qiladi',
  // Why a device could not be reached (R1, R1a: ADR-0052, ADR-0068).
  'lan.unreachable.local_network_denied': 'Telefon Vutric’ga mahalliy tarmoqni bermadi: Sozlamalarda ruxsat bering',
  'lan.unreachable.isolated': 'Bu Wi-Fi qurilmalarni bir-biridan ajratib qoʻyadi',
  'lan.unreachable.other_network': 'Qurilma boshqa tarmoqda',
  'lan.unreachable.no_network': 'Telefon hech qanday tarmoqqa ulanmagan',
  'lan.unreachable.captive': 'Bu Wi-Fi avval kirish sahifasini talab qiladi',
  'lan.captive.body':
      'Mehmonxona, aeroport yoki mehmon Wi-Fi’sida avval brauzerda tarmoqqa kiring, keyin qayta urinib koʻring.',
  'lan.next.fix_permission': 'Ruxsat berish',
  'lan.next.relay': 'Internet orqali ulanish',
  'lan.next.hotspot': 'Telefon hotspoti',
  'lan.next.cable': 'Kabel bilan',
  'lan.next.sign_in': 'Wi-Fi’ga kirish',
  // Big files through the relay (Rb, ADR-0068).
  'bulk.ask.title': 'Relay orqali {size}',
  'bulk.ask.body': 'Hozir yoʻl relay orqali: katta fayl sekin boradi. Telefon hotspoti yoki kabel ancha tez.',
  'bulk.ask.go': 'Baribir yuborish',
  'bulk.too_big': 'Relay orqali 10 GB dan koʻp yuborilmaydi: telefon hotspoti yoki kabelni ishlating',
  'bulk.hotspot': 'Hotspot bilan',
};

const Map<String, String> _en = {
  'route.picker.auto': 'Automatic',
  'route.picker.lan_only': 'This network only',
  'route.picker.internet': 'Over the internet',
  'route.picker.offline': 'No network',
  'route.picker.bluetooth': 'Bluetooth',
  'route.badge.direct_lan': 'Direct · LAN',
  'route.badge.direct_internet': 'Direct · internet',
  'route.badge.relay': 'Through a relay',
  'route.badge.hotspot': 'Hotspot',
  'route.badge.cable': 'Cable',
  'route.badge.connecting': 'Choosing a path…',
  'privacy.encrypted': 'Files are encrypted; only you and the receiver can see them',
  'privacy.nothing_leaves': 'Nothing leaves this network',
  'find.silent': 'No device answered',
  'find.denied': 'The phone blocked the local network',
  'find.unreadable': 'Not a code or an address',
};

/// The UI language: Uzbek unless the phone is set to English. Tests may pin it.
String uiLanguage = PlatformDispatcher.instance.locale.languageCode == 'en' ? 'en' : 'uz';

/// The line for [key], with `{name}` placeholders filled from [args]; the key itself if no table has it (so a
/// missing line is visible in testing, never an empty widget).
String tr(String key, [Map<String, String> args = const {}]) {
  var s = (uiLanguage == 'en' ? (_en[key] ?? _uz[key]) : (_uz[key] ?? _en[key])) ?? key;
  for (final e in args.entries) {
    s = s.replaceAll('{${e.key}}', e.value);
  }
  return s;
}

/// Whether [key] has a line (Rust errors that carry a key are shown through it).
bool hasLine(String key) => _uz.containsKey(key) || _en.containsKey(key);

/// Every key, for the test that checks the Uzbek table is complete.
Iterable<String> get allKeys => {..._uz.keys, ..._en.keys};
bool hasUzbek(String key) => _uz.containsKey(key);
