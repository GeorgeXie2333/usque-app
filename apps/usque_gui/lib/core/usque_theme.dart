import 'package:flutter/material.dart';

import 'usque_motion.dart';

/// Typeface roles.
///
/// Space Grotesk carries display copy and readouts, Manrope carries the
/// interface, IBM Plex Mono carries machine values such as addresses, ports,
/// and keys. CJK, Hangul, and Arabic-script glyphs fall through to platform
/// families.
class UsqueFonts {
  const UsqueFonts._();

  static const String display = 'SpaceGrotesk';
  static const String body = 'Manrope';
  static const String mono = 'IBMPlexMono';

  static const List<String> fallback = <String>[
    'Microsoft YaHei UI',
    'Microsoft YaHei',
    'Microsoft JhengHei UI',
    'Microsoft JhengHei',
    'Yu Gothic UI',
    'Malgun Gothic',
    'Segoe UI',
    'Tahoma',
    'Noto Sans CJK SC',
    'Noto Sans SC',
    'Noto Sans CJK TC',
    'Noto Sans TC',
    'Noto Sans JP',
    'Noto Sans KR',
    'Noto Naskh Arabic',
    'Noto Sans Arabic',
    'Leelawadee UI',
    'Noto Sans Thai',
    'Thonburi',
    'Source Han Sans SC',
    'PingFang SC',
    'PingFang TC',
    'PingFang HK',
    'Hiragino Sans',
    'Apple SD Gothic Neo',
    'Geeza Pro',
  ];

  static const List<String> monoFallback = <String>[
    'Cascadia Mono',
    'Consolas',
    'Roboto Mono',
    'monospace',
  ];
}

/// Corner radii. Tighter than Material defaults so panels read as instrument
/// plates rather than pill-shaped cards.
class UsqueRadii {
  const UsqueRadii._();

  static const double card = 18;
  static const double control = 12;
  static const double chip = 10;
  static const double pill = 999;
}

class UsqueColors {
  const UsqueColors._();

  /// Brand mark only: the logo, the onboarding pane, the scanning arc.
  static const Color orange = Color(0xFFF48120);

  /// Interactive accent, light theme. Reaches 4.5:1 on white.
  static const Color ember = Color(0xFFC2500C);

  /// Interactive accent, dark theme.
  static const Color emberLight = Color(0xFFFFA45C);

  static const Color ink = Color(0xFF1C1B18);
  static const Color canvas = Color(0xFFF5F4F1);
  static const Color canvasDark = Color(0xFF0E0E10);

  static const Color success = Color(0xFF128257);
  static const Color successLight = Color(0xFF4ADE9C);
  static const Color caution = Color(0xFFA15C00);
  static const Color cautionLight = Color(0xFFF2B24C);
  static const Color danger = Color(0xFFB3261E);
  static const Color dangerLight = Color(0xFFFFB4AB);

  static const Color inbound = Color(0xFF17708C);
  static const Color inboundLight = Color(0xFF7FCEE8);
  static const Color outbound = Color(0xFFA8480A);
  static const Color outboundLight = Color(0xFFFFB783);

  /// Legacy alias kept for call sites that still reference the deep orange.
  static const Color deepOrange = ember;

  /// Legacy alias; new code should use [caution].
  static const Color warning = caution;
}

/// Semantic colours that Material's [ColorScheme] has no slot for.
@immutable
class UsqueTokens extends ThemeExtension<UsqueTokens> {
  const UsqueTokens({
    required this.canvas,
    required this.hairline,
    required this.hairlineStrong,
    required this.brand,
    required this.brandSoft,
    required this.success,
    required this.caution,
    required this.danger,
    required this.inbound,
    required this.outbound,
    required this.ringTrack,
    required this.tint,
  });

  /// Page background behind every panel.
  final Color canvas;

  /// Default 1px separator and panel border.
  final Color hairline;

  /// Border for hovered panels and outlined controls.
  final Color hairlineStrong;

  /// The Usque orange, unchanged across themes.
  final Color brand;

  /// Brand wash for decorative fills.
  final Color brandSoft;

  final Color success;
  final Color caution;
  final Color danger;

  /// Download metric.
  final Color inbound;

  /// Upload metric.
  final Color outbound;

  /// Unlit bezel ticks on the connection ring.
  final Color ringTrack;

  /// Alpha used for soft status fills, so tints stay consistent app-wide.
  final double tint;

  static UsqueTokens of(BuildContext context) {
    final UsqueTokens? tokens = Theme.of(context).extension<UsqueTokens>();
    assert(
      tokens != null,
      'UsqueTokens missing from Theme; use UsqueTheme.light/dark',
    );
    return tokens ?? _light;
  }

  @override
  UsqueTokens copyWith({
    Color? canvas,
    Color? hairline,
    Color? hairlineStrong,
    Color? brand,
    Color? brandSoft,
    Color? success,
    Color? caution,
    Color? danger,
    Color? inbound,
    Color? outbound,
    Color? ringTrack,
    double? tint,
  }) {
    return UsqueTokens(
      canvas: canvas ?? this.canvas,
      hairline: hairline ?? this.hairline,
      hairlineStrong: hairlineStrong ?? this.hairlineStrong,
      brand: brand ?? this.brand,
      brandSoft: brandSoft ?? this.brandSoft,
      success: success ?? this.success,
      caution: caution ?? this.caution,
      danger: danger ?? this.danger,
      inbound: inbound ?? this.inbound,
      outbound: outbound ?? this.outbound,
      ringTrack: ringTrack ?? this.ringTrack,
      tint: tint ?? this.tint,
    );
  }

  @override
  UsqueTokens lerp(covariant UsqueTokens? other, double t) {
    if (other == null) return this;
    return UsqueTokens(
      canvas: Color.lerp(canvas, other.canvas, t)!,
      hairline: Color.lerp(hairline, other.hairline, t)!,
      hairlineStrong: Color.lerp(hairlineStrong, other.hairlineStrong, t)!,
      brand: Color.lerp(brand, other.brand, t)!,
      brandSoft: Color.lerp(brandSoft, other.brandSoft, t)!,
      success: Color.lerp(success, other.success, t)!,
      caution: Color.lerp(caution, other.caution, t)!,
      danger: Color.lerp(danger, other.danger, t)!,
      inbound: Color.lerp(inbound, other.inbound, t)!,
      outbound: Color.lerp(outbound, other.outbound, t)!,
      ringTrack: Color.lerp(ringTrack, other.ringTrack, t)!,
      tint: t < 0.5 ? tint : other.tint,
    );
  }

  static const UsqueTokens _light = UsqueTokens(
    canvas: UsqueColors.canvas,
    hairline: Color(0xFFE4E1DA),
    hairlineStrong: Color(0xFFCFCABF),
    brand: UsqueColors.orange,
    brandSoft: Color(0xFFFFEDDD),
    success: UsqueColors.success,
    caution: UsqueColors.caution,
    danger: UsqueColors.danger,
    inbound: UsqueColors.inbound,
    outbound: UsqueColors.outbound,
    ringTrack: Color(0xFFD8D4CB),
    tint: 0.12,
  );

  static const UsqueTokens _dark = UsqueTokens(
    canvas: UsqueColors.canvasDark,
    hairline: Color(0xFF2A2A2F),
    hairlineStrong: Color(0xFF3C3C43),
    brand: UsqueColors.orange,
    brandSoft: Color(0xFF2A1A0E),
    success: UsqueColors.successLight,
    caution: UsqueColors.cautionLight,
    danger: UsqueColors.dangerLight,
    inbound: UsqueColors.inboundLight,
    outbound: UsqueColors.outboundLight,
    ringTrack: Color(0xFF35353B),
    tint: 0.16,
  );
}

class UsqueTheme {
  const UsqueTheme._();

  static ThemeData light() => _build(_lightScheme, UsqueTokens._light);

  static ThemeData dark() => _build(_darkScheme, UsqueTokens._dark);

  static const ColorScheme _lightScheme = ColorScheme(
    brightness: Brightness.light,
    primary: UsqueColors.ember,
    onPrimary: Color(0xFFFFFFFF),
    primaryContainer: Color(0xFFFFE7D6),
    onPrimaryContainer: Color(0xFF5C2200),
    secondary: Color(0xFF7A5A3C),
    onSecondary: Color(0xFFFFFFFF),
    secondaryContainer: Color(0xFFF3E8DC),
    onSecondaryContainer: Color(0xFF3B2712),
    tertiary: UsqueColors.inbound,
    onTertiary: Color(0xFFFFFFFF),
    tertiaryContainer: Color(0xFFD5EAF2),
    onTertiaryContainer: Color(0xFF06364A),
    error: UsqueColors.danger,
    onError: Color(0xFFFFFFFF),
    errorContainer: Color(0xFFFBE6E4),
    onErrorContainer: Color(0xFF7A1710),
    surface: Color(0xFFFFFFFF),
    onSurface: UsqueColors.ink,
    onSurfaceVariant: Color(0xFF5D5A52),
    surfaceDim: Color(0xFFE7E4DD),
    surfaceBright: Color(0xFFFFFFFF),
    surfaceContainerLowest: Color(0xFFFFFFFF),
    surfaceContainerLow: Color(0xFFFAF9F6),
    surfaceContainer: Color(0xFFF3F1EC),
    surfaceContainerHigh: Color(0xFFEDEBE4),
    surfaceContainerHighest: Color(0xFFE7E4DC),
    outline: Color(0xFF8E8A80),
    outlineVariant: Color(0xFFE4E1DA),
    shadow: Color(0xFF000000),
    scrim: Color(0xFF000000),
    inverseSurface: Color(0xFF31302C),
    onInverseSurface: Color(0xFFF5F3EE),
    inversePrimary: Color(0xFFFFB783),
    surfaceTint: Color(0x00000000),
  );

  static const ColorScheme _darkScheme = ColorScheme(
    brightness: Brightness.dark,
    primary: UsqueColors.emberLight,
    onPrimary: Color(0xFF441800),
    primaryContainer: Color(0xFF6B2C00),
    onPrimaryContainer: Color(0xFFFFDBC4),
    secondary: Color(0xFFD9C3AC),
    onSecondary: Color(0xFF3B2712),
    secondaryContainer: Color(0xFF2E241A),
    onSecondaryContainer: Color(0xFFF3E8DC),
    tertiary: UsqueColors.inboundLight,
    onTertiary: Color(0xFF00363F),
    tertiaryContainer: Color(0xFF124E5F),
    onTertiaryContainer: Color(0xFFCDEBF6),
    error: UsqueColors.dangerLight,
    onError: Color(0xFF690005),
    errorContainer: Color(0xFF6B1F1A),
    onErrorContainer: Color(0xFFFFDAD6),
    surface: Color(0xFF18181A),
    onSurface: Color(0xFFECEAE6),
    onSurfaceVariant: Color(0xFFA5A29B),
    surfaceDim: Color(0xFF0E0E10),
    surfaceBright: Color(0xFF2A2A2E),
    surfaceContainerLowest: Color(0xFF121213),
    surfaceContainerLow: Color(0xFF1A1A1D),
    surfaceContainer: Color(0xFF1F1F23),
    surfaceContainerHigh: Color(0xFF26262A),
    surfaceContainerHighest: Color(0xFF2D2D32),
    outline: Color(0xFF6E6B65),
    outlineVariant: Color(0xFF2A2A2F),
    shadow: Color(0xFF000000),
    scrim: Color(0xFF000000),
    inverseSurface: Color(0xFFECEAE6),
    onInverseSurface: Color(0xFF1C1B18),
    inversePrimary: UsqueColors.ember,
    surfaceTint: Color(0x00000000),
  );

  /// Machine values: addresses, ports, keys, identifiers.
  static TextStyle mono(
    BuildContext context, {
    double? size,
    FontWeight weight = FontWeight.w400,
    Color? color,
  }) {
    return TextStyle(
      fontFamily: UsqueFonts.mono,
      fontFamilyFallback: UsqueFonts.monoFallback,
      fontSize: size ?? 13,
      fontWeight: weight,
      height: 1.35,
      letterSpacing: 0,
      color: color ?? Theme.of(context).colorScheme.onSurface,
    );
  }

  static const TextTheme _textTheme = TextTheme(
    displayLarge: TextStyle(
      fontFamily: UsqueFonts.display,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 44,
      fontWeight: FontWeight.w600,
      letterSpacing: -1.6,
      height: 1.05,
    ),
    displayMedium: TextStyle(
      fontFamily: UsqueFonts.display,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 36,
      fontWeight: FontWeight.w600,
      letterSpacing: -1.2,
      height: 1.08,
    ),
    displaySmall: TextStyle(
      fontFamily: UsqueFonts.display,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 29,
      fontWeight: FontWeight.w600,
      letterSpacing: -0.9,
      height: 1.1,
    ),
    headlineLarge: TextStyle(
      fontFamily: UsqueFonts.display,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 26,
      fontWeight: FontWeight.w600,
      letterSpacing: -0.7,
      height: 1.14,
    ),
    headlineMedium: TextStyle(
      fontFamily: UsqueFonts.display,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 23,
      fontWeight: FontWeight.w600,
      letterSpacing: -0.5,
      height: 1.18,
    ),
    headlineSmall: TextStyle(
      fontFamily: UsqueFonts.display,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 19,
      fontWeight: FontWeight.w600,
      letterSpacing: -0.3,
      height: 1.2,
    ),
    titleLarge: TextStyle(
      fontFamily: UsqueFonts.display,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 17,
      fontWeight: FontWeight.w600,
      letterSpacing: -0.2,
      height: 1.25,
    ),
    titleMedium: TextStyle(
      fontFamily: UsqueFonts.body,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 14.5,
      fontWeight: FontWeight.w700,
      letterSpacing: -0.05,
      height: 1.3,
    ),
    titleSmall: TextStyle(
      fontFamily: UsqueFonts.body,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 13,
      fontWeight: FontWeight.w600,
      letterSpacing: 0,
      height: 1.3,
    ),
    bodyLarge: TextStyle(
      fontFamily: UsqueFonts.body,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 14.5,
      fontWeight: FontWeight.w400,
      height: 1.5,
    ),
    bodyMedium: TextStyle(
      fontFamily: UsqueFonts.body,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 13,
      fontWeight: FontWeight.w400,
      height: 1.45,
    ),
    bodySmall: TextStyle(
      fontFamily: UsqueFonts.body,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 12,
      fontWeight: FontWeight.w400,
      height: 1.4,
    ),
    labelLarge: TextStyle(
      fontFamily: UsqueFonts.body,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 13.5,
      fontWeight: FontWeight.w600,
      letterSpacing: 0.1,
      height: 1.2,
    ),
    labelMedium: TextStyle(
      fontFamily: UsqueFonts.body,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 12,
      fontWeight: FontWeight.w600,
      letterSpacing: 0.2,
      height: 1.2,
    ),
    labelSmall: TextStyle(
      fontFamily: UsqueFonts.body,
      fontFamilyFallback: UsqueFonts.fallback,
      fontSize: 10.5,
      fontWeight: FontWeight.w700,
      letterSpacing: 0.9,
      height: 1.2,
    ),
  );

  static ThemeData _build(ColorScheme scheme, UsqueTokens tokens) {
    final TextTheme text = _textTheme.apply(
      bodyColor: scheme.onSurface,
      displayColor: scheme.onSurface,
    );

    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      brightness: scheme.brightness,
      scaffoldBackgroundColor: tokens.canvas,
      canvasColor: tokens.canvas,
      splashFactory: InkSparkle.splashFactory,
      visualDensity: VisualDensity.standard,
      textTheme: text,
      extensions: <ThemeExtension<dynamic>>[tokens],
      dividerColor: tokens.hairline,
      dividerTheme: DividerThemeData(
        color: tokens.hairline,
        thickness: 1,
        space: 1,
      ),
      cardTheme: CardThemeData(
        color: scheme.surface,
        elevation: 0,
        margin: EdgeInsets.zero,
        clipBehavior: Clip.antiAlias,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(UsqueRadii.card),
          side: BorderSide(color: tokens.hairline),
        ),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: scheme.surfaceContainerLow,
        hintStyle: text.bodyMedium?.copyWith(color: scheme.onSurfaceVariant),
        labelStyle: text.bodyMedium?.copyWith(color: scheme.onSurfaceVariant),
        floatingLabelStyle: text.labelMedium?.copyWith(color: scheme.primary),
        helperStyle: text.bodySmall?.copyWith(color: scheme.onSurfaceVariant),
        errorStyle: text.bodySmall?.copyWith(color: scheme.error),
        contentPadding: const EdgeInsets.symmetric(
          horizontal: 14,
          vertical: 14,
        ),
        border: _fieldBorder(tokens.hairline),
        enabledBorder: _fieldBorder(tokens.hairline),
        disabledBorder: _fieldBorder(tokens.hairline),
        focusedBorder: _fieldBorder(scheme.primary, width: 1.6),
        errorBorder: _fieldBorder(scheme.error),
        focusedErrorBorder: _fieldBorder(scheme.error, width: 1.6),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          minimumSize: const Size(44, 44),
          padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 12),
          textStyle: text.labelLarge,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(UsqueRadii.control),
          ),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          minimumSize: const Size(44, 44),
          padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 12),
          textStyle: text.labelLarge,
          foregroundColor: scheme.onSurface,
          side: BorderSide(color: tokens.hairlineStrong),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(UsqueRadii.control),
          ),
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          minimumSize: const Size(44, 44),
          padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 10),
          textStyle: text.labelLarge,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(UsqueRadii.chip),
          ),
        ),
      ),
      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(UsqueRadii.chip),
          ),
        ),
      ),
      segmentedButtonTheme: SegmentedButtonThemeData(
        style: SegmentedButton.styleFrom(
          textStyle: text.labelLarge,
          side: BorderSide(color: tokens.hairlineStrong),
          selectedBackgroundColor: scheme.primary.withValues(
            alpha: tokens.tint,
          ),
          selectedForegroundColor: scheme.primary,
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(UsqueRadii.control),
          ),
        ),
      ),
      navigationRailTheme: NavigationRailThemeData(
        backgroundColor: tokens.canvas,
        indicatorColor: scheme.primary.withValues(alpha: tokens.tint),
        indicatorShape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(UsqueRadii.control),
        ),
        selectedIconTheme: IconThemeData(color: scheme.primary, size: 21),
        unselectedIconTheme: IconThemeData(
          color: scheme.onSurfaceVariant,
          size: 21,
        ),
        selectedLabelTextStyle: text.labelLarge?.copyWith(
          color: scheme.onSurface,
          fontWeight: FontWeight.w700,
        ),
        unselectedLabelTextStyle: text.labelLarge?.copyWith(
          color: scheme.onSurfaceVariant,
        ),
        useIndicator: true,
      ),
      navigationBarTheme: NavigationBarThemeData(
        height: 66,
        backgroundColor: scheme.surface,
        surfaceTintColor: Colors.transparent,
        elevation: 0,
        indicatorColor: scheme.primary.withValues(alpha: tokens.tint),
        indicatorShape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(UsqueRadii.chip),
        ),
        labelBehavior: NavigationDestinationLabelBehavior.alwaysShow,
        labelTextStyle: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.selected)
              ? text.labelMedium?.copyWith(
                  color: scheme.onSurface,
                  fontWeight: FontWeight.w700,
                )
              : text.labelMedium?.copyWith(color: scheme.onSurfaceVariant),
        ),
        iconTheme: WidgetStateProperty.resolveWith(
          (states) => IconThemeData(
            size: 21,
            color: states.contains(WidgetState.selected)
                ? scheme.primary
                : scheme.onSurfaceVariant,
          ),
        ),
      ),
      switchTheme: SwitchThemeData(
        thumbColor: WidgetStateProperty.resolveWith(
          (states) =>
              states.contains(WidgetState.selected) ? scheme.onPrimary : null,
        ),
        trackColor: WidgetStateProperty.resolveWith(
          (states) =>
              states.contains(WidgetState.selected) ? scheme.primary : null,
        ),
        trackOutlineColor: WidgetStateProperty.resolveWith(
          (states) => states.contains(WidgetState.selected)
              ? Colors.transparent
              : tokens.hairlineStrong,
        ),
      ),
      checkboxTheme: CheckboxThemeData(
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(5)),
        side: BorderSide(color: tokens.hairlineStrong, width: 1.6),
      ),
      listTileTheme: ListTileThemeData(
        iconColor: scheme.onSurfaceVariant,
        titleTextStyle: text.bodyLarge,
        subtitleTextStyle: text.bodySmall?.copyWith(
          color: scheme.onSurfaceVariant,
        ),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(UsqueRadii.chip),
        ),
      ),
      dialogTheme: DialogThemeData(
        backgroundColor: scheme.surface,
        surfaceTintColor: Colors.transparent,
        elevation: 0,
        titleTextStyle: text.headlineSmall,
        contentTextStyle: text.bodyMedium,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(UsqueRadii.card),
          side: BorderSide(color: tokens.hairline),
        ),
      ),
      menuTheme: MenuThemeData(
        style: MenuStyle(
          backgroundColor: WidgetStatePropertyAll<Color>(scheme.surface),
          surfaceTintColor: const WidgetStatePropertyAll<Color>(
            Colors.transparent,
          ),
          shape: WidgetStatePropertyAll<OutlinedBorder>(
            RoundedRectangleBorder(
              borderRadius: BorderRadius.circular(UsqueRadii.control),
              side: BorderSide(color: tokens.hairline),
            ),
          ),
        ),
      ),
      popupMenuTheme: PopupMenuThemeData(
        color: scheme.surface,
        surfaceTintColor: Colors.transparent,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(UsqueRadii.control),
          side: BorderSide(color: tokens.hairline),
        ),
      ),
      snackBarTheme: SnackBarThemeData(
        behavior: SnackBarBehavior.floating,
        backgroundColor: scheme.inverseSurface,
        contentTextStyle: text.bodyMedium?.copyWith(
          color: scheme.onInverseSurface,
        ),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(UsqueRadii.control),
        ),
      ),
      tooltipTheme: TooltipThemeData(
        waitDuration: const Duration(milliseconds: 400),
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        textStyle: text.bodySmall?.copyWith(color: scheme.onInverseSurface),
        decoration: BoxDecoration(
          color: scheme.inverseSurface,
          borderRadius: BorderRadius.circular(8),
        ),
      ),
      appBarTheme: AppBarTheme(
        backgroundColor: tokens.canvas,
        surfaceTintColor: Colors.transparent,
        foregroundColor: scheme.onSurface,
        elevation: 0,
        scrolledUnderElevation: 0,
        centerTitle: false,
        titleTextStyle: text.headlineSmall,
      ),
      progressIndicatorTheme: ProgressIndicatorThemeData(
        color: scheme.primary,
        linearTrackColor: tokens.hairline,
        circularTrackColor: Colors.transparent,
      ),
      scrollbarTheme: ScrollbarThemeData(
        thickness: const WidgetStatePropertyAll<double>(8),
        radius: const Radius.circular(8),
        thumbColor: WidgetStatePropertyAll<Color>(
          scheme.onSurfaceVariant.withValues(alpha: 0.28),
        ),
      ),
      pageTransitionsTheme: const PageTransitionsTheme(
        builders: <TargetPlatform, PageTransitionsBuilder>{
          TargetPlatform.android: UsquePageTransitionsBuilder(),
          TargetPlatform.windows: UsquePageTransitionsBuilder(),
          TargetPlatform.linux: UsquePageTransitionsBuilder(),
          TargetPlatform.macOS: UsquePageTransitionsBuilder(),
          TargetPlatform.iOS: UsquePageTransitionsBuilder(),
          TargetPlatform.fuchsia: UsquePageTransitionsBuilder(),
        },
      ),
    );
  }

  static OutlineInputBorder _fieldBorder(Color color, {double width = 1}) {
    return OutlineInputBorder(
      borderRadius: BorderRadius.circular(UsqueRadii.control),
      borderSide: BorderSide(color: color, width: width),
    );
  }
}
