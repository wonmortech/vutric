// Session plumbing and error text shared by the screens (the look itself is design/tokens.dart).

import 'dart:io';

import 'i18n.dart';
import 'src/rust/api.dart' as vutric;

Future<void> _lastDisconnect = Future.value();

/// Closes the session and cancels every waiting call. A screen's dispose cannot await it, so calls started
/// through [afterDisconnect] wait for it instead: a page closing can never cancel the next page's connect.
Future<void> disconnectNow() => _lastDisconnect = vutric.disconnect().catchError((Object _) {});

/// Runs [job] once the latest [disconnectNow] has finished.
Future<T> afterDisconnect<T>(Future<T> Function() job) async {
  await _lastDisconnect;
  return job();
}

/// iPhone cannot use mDNS without Apple's multicast entitlement (a free Apple ID has none): pair by QR there.
final hasMdns = !Platform.isIOS;

/// How the session travels, for the line under the peer's name: the route badge of ADR-0060 when Rust gave one
/// (`route.badge.*`), else the older direct/relay word.
String routeLabel(String route) => switch (route) {
  final k when k.startsWith('route.badge.') => tr(k),
  'direct' => "to'g'ridan-to'g'ri",
  'relay' => 'internet orqali (shifrlangan)',
  _ => tr('route.badge.connecting'),
};

/// A call that `disconnect()` cancelled on purpose (the page closed, or the user tapped cancel).
bool isCancelled(Object e) => '$e'.contains('vutric: cancelled');

/// The Rust error, trimmed to its first line for the screen.
String errorText(Object e) {
  // Dart's replaceFirst has no $1 back-references (it would print a literal "$1"): take the group by hand.
  final raw = '$e';
  final s = RegExp(r'^AnyhowException\((.*)\)$', dotAll: true).firstMatch(raw)?.group(1) ?? raw;
  final line = s.split('\n').first.trim();
  if (hasLine(line)) return tr(line);
  if (line.contains('accepts only its trusted devices')) {
    return 'Qurilma sizni tanimaydi: avval QR orqali juftlang yoki u yerda “Hamma, 10 daqiqa”ni yoqing';
  }
  return line.isEmpty ? 'Nomaʼlum xato' : line;
}
