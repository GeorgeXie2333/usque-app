import 'package:flutter/material.dart';
import 'package:flutter_svg/flutter_svg.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';
import '../models/chain_exit_models.dart';

class ChainSourceIcon extends StatelessWidget {
  const ChainSourceIcon({
    required this.source,
    this.size = 24,
    this.color,
    super.key,
  });
  final ChainSource source;
  final double size;
  final Color? color;
  @override
  Widget build(BuildContext context) {
    final tint =
        color ??
        IconTheme.of(context).color ??
        Theme.of(context).colorScheme.onSurface;
    if (source == ChainSource.vpnGate) {
      return Icon(LucideIcons.globe, size: size, color: tint);
    }
    return SvgPicture.asset(
      'assets/icons/${source == ChainSource.openvpnCustom ? 'openvpn' : 'wireguard'}.svg',
      width: size,
      height: size,
      colorFilter: ColorFilter.mode(tint, BlendMode.srcIn),
      excludeFromSemantics: true,
    );
  }
}
