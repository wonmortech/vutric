// Shared look and error handling for the spike screens.

import 'dart:io';

import 'package:material_ui/material_ui.dart';

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

const cobalt = Color(0xFF062B8A);
const signal = Color(0xFF2F6BFF);
const sand = Color(0xFFE6DBC7);
const paper = Color(0xFFF7F2E9);
const ok = Color(0xFF1D7A4E);
const err = Color(0xFFA8321F);

/// iPhone cannot use mDNS without Apple's multicast entitlement (a free Apple ID has none): pair by QR there.
final hasMdns = !Platform.isIOS;

/// A call that `disconnect()` cancelled on purpose (the page closed, or the user tapped cancel).
bool isCancelled(Object e) => '$e'.contains('vutric: cancelled');

/// The Rust error, trimmed to its first line for the screen.
String errorText(Object e) {
  final s = '$e'.replaceFirst(RegExp(r'^AnyhowException\((.*)\)$', dotAll: true), r'$1');
  final line = s.split('\n').first.trim();
  return line.isEmpty ? 'Nomaʼlum xato' : line;
}
