import 'package:flutter/material.dart';

/// Shared brand artwork. The containing heading normally supplies semantics.
class UsqueLogo extends StatelessWidget {
  const UsqueLogo({required this.size, this.semanticLabel, super.key});

  static const String lightAsset = 'assets/branding/usque-ui-icon.png';
  static const String darkAsset = 'assets/branding/usque-ui-icon-dark.png';

  static String assetFor(Brightness brightness) =>
      brightness == Brightness.dark ? darkAsset : lightAsset;

  final double size;
  final String? semanticLabel;

  @override
  Widget build(BuildContext context) => Image.asset(
    assetFor(Theme.of(context).brightness),
    width: size,
    height: size,
    filterQuality: FilterQuality.medium,
    excludeFromSemantics: semanticLabel == null,
    semanticLabel: semanticLabel,
  );
}
