import 'package:flutter/material.dart';

class UsqueColors {
  static const Color orange = Color(0xFFF48120);
  static const Color deepOrange = Color(0xFFA33A00);
  static const Color ink = Color(0xFF191C1E);
  static const Color canvas = Color(0xFFF7F7F5);
  static const Color success = Color(0xFF17734A);
  static const Color warning = Color(0xFF9A4A00);
  static const Color danger = Color(0xFFB3261E);
}

class UsqueTheme {
  const UsqueTheme._();

  static ThemeData light() {
    final scheme = ColorScheme.fromSeed(
      seedColor: UsqueColors.orange,
      brightness: Brightness.light,
      primary: UsqueColors.deepOrange,
      secondary: UsqueColors.orange,
      surface: Colors.white,
      onSurface: UsqueColors.ink,
      error: UsqueColors.danger,
    );
    return _base(scheme).copyWith(
      scaffoldBackgroundColor: UsqueColors.canvas,
      cardTheme: const CardThemeData(
        color: Colors.white,
        elevation: 0,
        margin: EdgeInsets.zero,
      ),
    );
  }

  static ThemeData dark() {
    final scheme = ColorScheme.fromSeed(
      seedColor: UsqueColors.orange,
      brightness: Brightness.dark,
      primary: const Color(0xFFFFB783),
      secondary: const Color(0xFFFFB06E),
      surface: const Color(0xFF1A1C1E),
      onSurface: const Color(0xFFE4E2E0),
      error: const Color(0xFFFFB4AB),
    );
    return _base(scheme).copyWith(
      scaffoldBackgroundColor: const Color(0xFF111315),
      cardTheme: const CardThemeData(
        color: Color(0xFF1A1C1E),
        elevation: 0,
        margin: EdgeInsets.zero,
      ),
    );
  }

  static ThemeData _base(ColorScheme scheme) {
    final outline = scheme.outlineVariant.withValues(alpha: 0.72);
    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      visualDensity: VisualDensity.standard,
      textTheme: const TextTheme(
        displaySmall: TextStyle(
          fontSize: 36,
          fontWeight: FontWeight.w700,
          letterSpacing: -1.2,
        ),
        headlineMedium: TextStyle(
          fontSize: 28,
          fontWeight: FontWeight.w700,
          letterSpacing: -0.6,
        ),
        titleLarge: TextStyle(fontSize: 20, fontWeight: FontWeight.w700),
        titleMedium: TextStyle(fontSize: 16, fontWeight: FontWeight.w600),
        bodyLarge: TextStyle(fontSize: 16, height: 1.45),
        bodyMedium: TextStyle(fontSize: 14, height: 1.4),
        labelLarge: TextStyle(fontSize: 14, fontWeight: FontWeight.w600),
      ),
      dividerColor: outline,
      cardTheme: CardThemeData(
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(24),
          side: BorderSide(color: outline),
        ),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: scheme.surfaceContainerLowest,
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(14),
          borderSide: BorderSide(color: outline),
        ),
        enabledBorder: OutlineInputBorder(
          borderRadius: BorderRadius.circular(14),
          borderSide: BorderSide(color: outline),
        ),
        contentPadding: const EdgeInsets.symmetric(
          horizontal: 16,
          vertical: 15,
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          minimumSize: const Size(48, 48),
          padding: const EdgeInsets.symmetric(horizontal: 20, vertical: 14),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(14),
          ),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          minimumSize: const Size(48, 48),
          padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 13),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(14),
          ),
        ),
      ),
      navigationRailTheme: NavigationRailThemeData(
        backgroundColor: scheme.surface,
        indicatorColor: scheme.secondaryContainer,
        selectedIconTheme: IconThemeData(color: scheme.onSecondaryContainer),
        selectedLabelTextStyle: TextStyle(
          color: scheme.onSurface,
          fontWeight: FontWeight.w700,
        ),
      ),
      navigationBarTheme: NavigationBarThemeData(
        backgroundColor: scheme.surface,
        indicatorColor: scheme.secondaryContainer,
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
      ),
      snackBarTheme: SnackBarThemeData(
        behavior: SnackBarBehavior.floating,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(14)),
      ),
    );
  }
}
