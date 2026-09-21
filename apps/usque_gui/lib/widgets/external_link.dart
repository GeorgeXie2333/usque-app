import 'package:flutter/material.dart';
import 'package:url_launcher/url_launcher.dart';

import '../core/app_strings.dart';

Future<bool> openExternalLink(
  BuildContext context,
  AppStrings strings,
  String url, {
  Future<bool> Function(Uri)? launcher,
}) async {
  if (!context.mounted) return false;
  try {
    if (await (launcher ?? _launch)(Uri.parse(url))) return true;
  } on Object {
    // Never expose raw platform messages or paths in browser failure feedback.
  }
  if (context.mounted) {
    ScaffoldMessenger.maybeOf(context)?.showSnackBar(
      SnackBar(content: Text(strings.get('zero_trust_browser_failed'))),
    );
  }
  return false;
}

Future<bool> _launch(Uri uri) =>
    launchUrl(uri, mode: LaunchMode.externalApplication);
