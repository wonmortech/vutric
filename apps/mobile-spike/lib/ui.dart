// Shared look and error handling for the spike screens.

import 'dart:io';

import 'package:material_ui/material_ui.dart';

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
