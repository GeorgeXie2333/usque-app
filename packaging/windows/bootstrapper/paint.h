#pragma once

// Anti-aliased shapes for the setup window's custom-drawn controls. Pure:
// shapes render into a top-down 32-bit DIB buffer that Win32 code blits,
// after which it draws text natively. Mirrors crates/usque-uninstall/src/paint.rs.

#include <windows.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <vector>

#include "setup_tokens.h"
#include "state.h"

namespace usque::setup::paint {

struct Rgb {
  std::uint8_t r = 0, g = 0, b = 0;

  static constexpr Rgb FromColorref(COLORREF value) {
    return {static_cast<std::uint8_t>(value & 0xFF), static_cast<std::uint8_t>((value >> 8) & 0xFF),
            static_cast<std::uint8_t>((value >> 16) & 0xFF)};
  }
  constexpr std::uint32_t Dib() const {
    return (static_cast<std::uint32_t>(r) << 16) | (static_cast<std::uint32_t>(g) << 8) | b;
  }
  constexpr COLORREF Colorref() const { return RGB(r, g, b); }
  // This color at `amount` opacity over `base`.
  Rgb Over(Rgb base, float amount) const {
    amount = std::clamp(amount, 0.0f, 1.0f);
    const auto channel = [amount](std::uint8_t top, std::uint8_t bottom) {
      return static_cast<std::uint8_t>(static_cast<float>(top) * amount + static_cast<float>(bottom) * (1.0f - amount) + 0.5f);
    };
    return {channel(r, base.r), channel(g, base.g), channel(b, base.b)};
  }
  friend constexpr bool operator==(Rgb, Rgb) = default;
};

inline constexpr Rgb kWhite{255, 255, 255};
// Peak white blend of the held-value highlight.
inline constexpr float kSweep = 0.3f;

struct Rect {
  float left = 0, top = 0, right = 0, bottom = 0;
  Rect Inset(float amount) const { return {left + amount, top + amount, right - amount, bottom - amount}; }
};

// Signed distance from a point to a rounded rectangle; negative inside.
inline float RoundedDistance(float x, float y, Rect rect, float radius) {
  const float half_width = (std::max)((rect.right - rect.left) / 2.0f, 0.0f);
  const float half_height = (std::max)((rect.bottom - rect.top) / 2.0f, 0.0f);
  radius = std::clamp(radius, 0.0f, (std::min)(half_width, half_height));
  const float qx = std::fabs(x - (rect.left + rect.right) / 2.0f) - half_width + radius;
  const float qy = std::fabs(y - (rect.top + rect.bottom) / 2.0f) - half_height + radius;
  return std::hypot((std::max)(qx, 0.0f), (std::max)(qy, 0.0f)) + (std::min)((std::max)(qx, qy), 0.0f) - radius;
}

inline float Coverage(float distance) { return std::clamp(0.5f - distance, 0.0f, 1.0f); }

// The part of pixel column [x, x + 1) inside [start, end).
inline float ColumnOverlap(float x, float start, float end) {
  return std::clamp((std::min)(x + 1.0f, end) - (std::max)(x, start), 0.0f, 1.0f);
}

inline float Smoothstep(float value) {
  value = std::clamp(value, 0.0f, 1.0f);
  return value * value * (3.0f - 2.0f * value);
}

class Canvas {
 public:
  Canvas(int width, int height, Rgb background)
      : width_((std::max)(width, 0)), height_((std::max)(height, 0)),
        pixels_(static_cast<size_t>(width_) * static_cast<size_t>(height_), background.Dib()) {}

  int Width() const { return width_; }
  int Height() const { return height_; }
  const std::vector<std::uint32_t>& Pixels() const { return pixels_; }
  // Reverses every row, for device contexts that mirror coordinates but not
  // DIB contents.
  void Mirror() {
    for (int y = 0; y < height_; ++y) {
      const auto row = pixels_.begin() + static_cast<std::ptrdiff_t>(y) * width_;
      std::reverse(row, row + width_);
    }
  }
  Rgb Pixel(int x, int y) const {
    const auto value = pixels_[static_cast<size_t>(y) * static_cast<size_t>(width_) + static_cast<size_t>(x)];
    return {static_cast<std::uint8_t>(value >> 16), static_cast<std::uint8_t>(value >> 8), static_cast<std::uint8_t>(value)};
  }

  // Blends shade(x, y, color&) -> coverage at every pixel a shape inside
  // `bounds` can reach.
  template <typename Shade>
  void Paint(Rect bounds, Shade shade) {
    const auto clamp = [](float value, int limit) { return (std::min)(static_cast<int>((std::max)(value, 0.0f)), limit); };
    const int left = clamp(std::floor(bounds.left) - 1.0f, width_), right = clamp(std::ceil(bounds.right) + 1.0f, width_);
    const int top = clamp(std::floor(bounds.top) - 1.0f, height_), bottom = clamp(std::ceil(bounds.bottom) + 1.0f, height_);
    for (int y = top; y < bottom; ++y) {
      for (int x = left; x < right; ++x) {
        Rgb color{};
        const float amount = shade(static_cast<float>(x), static_cast<float>(y), color);
        if (amount <= 0.0f) continue;
        auto& pixel = pixels_[static_cast<size_t>(y) * static_cast<size_t>(width_) + static_cast<size_t>(x)];
        pixel = color.Over(Pixel(x, y), amount).Dib();
      }
    }
  }

  void FillRounded(Rect rect, float radius, Rgb color) {
    Paint(rect, [&](float x, float y, Rgb& out) {
      out = color;
      return Coverage(RoundedDistance(x + 0.5f, y + 0.5f, rect, radius));
    });
  }

  // A stroke of `width` lying inside the rounded rectangle's edge.
  void StrokeRounded(Rect rect, float radius, float width, Rgb color) {
    Paint(rect, [&](float x, float y, Rgb& out) {
      out = color;
      const float distance = RoundedDistance(x + 0.5f, y + 0.5f, rect, radius);
      return Coverage(std::fabs(distance + width / 2.0f) - width / 2.0f);
    });
  }

  // A round-capped line segment.
  void Line(float x0, float y0, float x1, float y1, float width, Rgb color) {
    const Rect bounds{(std::min)(x0, x1) - width, (std::min)(y0, y1) - width,
                      (std::max)(x0, x1) + width, (std::max)(y0, y1) + width};
    const float dx = x1 - x0, dy = y1 - y0, length = dx * dx + dy * dy;
    Paint(bounds, [&](float x, float y, Rgb& out) {
      out = color;
      const float px = x + 0.5f - x0, py = y + 0.5f - y0;
      const float t = length > 0.0f ? std::clamp((px * dx + py * dy) / length, 0.0f, 1.0f) : 0.0f;
      return Coverage(std::hypot(px - dx * t, py - dy * t) - width / 2.0f);
    });
  }

 private:
  int width_, height_;
  std::vector<std::uint32_t> pixels_;
};

// Footer and inline button roles, matching Flutter's filled, outlined and
// text buttons.
enum class ButtonKind { filled, outlined, text };

struct ButtonLook {
  bool has_fill = false;
  Rgb fill{};
  bool has_border = false;
  Rgb border{};
  Rgb ink{};
  // Keyboard focus ring; it must contrast with the button's own fill.
  Rgb ring{};
};

// Button colors from the shared tokens. High contrast uses system colors in
// the Win32 layer instead.
inline ButtonLook ButtonLookFor(ButtonKind kind, bool enabled, bool pressed, const Tokens& tokens) {
  const auto color = [](COLORREF value) { return Rgb::FromColorref(value); };
  ButtonLook look;
  look.ink = color(enabled ? (kind == ButtonKind::filled ? tokens.on_primary :
    kind == ButtonKind::text ? tokens.primary : tokens.ink) : tokens.disabled_ink);
  if (kind == ButtonKind::filled) {
    look.has_fill = true;
    look.fill = color(!enabled ? tokens.disabled_fill : pressed ? tokens.primary_pressed : tokens.primary);
    look.ring = color(tokens.on_primary);
    return look;
  }
  look.has_fill = enabled && pressed;
  look.fill = color(tokens.pressed);
  look.has_border = kind == ButtonKind::outlined;
  look.border = color(enabled ? tokens.hairline_strong : tokens.hairline);
  look.ring = color(tokens.primary);
  return look;
}

enum class BarLookKind { waiting, filled, halted };

struct BarLook {
  BarLookKind kind = BarLookKind::waiting;
  float phase = 0.0f;     // waiting segment or held-highlight position, [0, 1)
  float fraction = 0.0f;  // presented fraction for filled and halted bars
  bool sweep = false;     // held-value activity highlight
  friend bool operator==(const BarLook&, const BarLook&) = default;
};

struct BarColors {
  Rgb background, track, fill, halted;
  bool has_outline = false;  // high contrast outlines the track instead of tinting it
  Rgb outline{};
};

// Renders a pill-shaped track with rounded ends. The fill grows from the left
// edge; a mirrored (RTL) device context mirrors the bitmap.
inline Canvas RenderBar(int width, int height, const BarLook& look, const BarColors& colors) {
  Canvas canvas(width, height, colors.background);
  const float w = static_cast<float>(canvas.Width()), h = static_cast<float>(canvas.Height());
  const Rect track{0.0f, 0.0f, w, h};
  const float radius = h / 2.0f;
  canvas.FillRounded(track, radius, colors.track);
  if (colors.has_outline) canvas.StrokeRounded(track, radius, 1.0f, colors.outline);
  const auto segment = [&](float start, float end, auto color_at) {
    canvas.Paint(track, [&](float x, float y, Rgb& out) {
      out = color_at(x + 0.5f);
      return Coverage(RoundedDistance(x + 0.5f, y + 0.5f, track, radius)) * ColumnOverlap(x, start, end);
    });
  };
  if (look.kind == BarLookKind::waiting) {
    const float length = (std::max)(w * 0.3f, h * 2.0f);
    const float start = -length + (w + length) * Smoothstep(look.phase);
    segment(start, start + length, [&](float) { return colors.fill; });
  } else if (look.kind == BarLookKind::filled) {
    const float end = w * std::clamp(look.fraction, 0.0f, 1.0f);
    const float band = (std::max)(w * 0.12f, h * 8.0f);
    const float center = -band + (end + 2.0f * band) * std::clamp(look.phase, 0.0f, 1.0f);
    segment(0.0f, end, [&](float x) {
      if (!look.sweep) return colors.fill;
      return kWhite.Over(colors.fill, kSweep * Smoothstep(1.0f - std::fabs(x - center) / band));
    });
  } else {
    segment(0.0f, w * std::clamp(look.fraction, 0.0f, 1.0f), [&](float) { return colors.halted; });
  }
  return canvas;
}

// Eases the presented fraction toward the model's value and derives the
// waiting and held animations. Pure: the caller supplies elapsed time.
class BarMotion {
 public:
  BarLook Step(const SetupProgress& bar, unsigned elapsed_ms, bool motion) {
    const unsigned scan = static_cast<unsigned>(kMotionScanMs);
    clock_ms_ = (clock_ms_ + elapsed_ms) % scan;
    const float phase = static_cast<float>(clock_ms_) / static_cast<float>(scan);
    if (bar.mode == BarMode::waiting) {
      shown_ = 0.0f; has_target_ = false; held_ms_ = 0;
      return {BarLookKind::waiting, motion ? phase : 0.5f, 0.0f, false};
    }
    const float target = static_cast<float>((std::min)(bar.value, kBarComplete)) / 1000.0f;
    if (has_target_ && target == target_) held_ms_ += elapsed_ms;
    else { held_ms_ = 0; target_ = target; has_target_ = true; }
    // A new attempt may start lower; only easing forward is animated.
    if (!motion || target < shown_) {
      shown_ = target;
    } else {
      const float tau = static_cast<float>(kMotionBaseMs) / 2.0f;
      shown_ += (target - shown_) * (1.0f - std::exp(-static_cast<float>(elapsed_ms) / tau));
      if (target - shown_ < 0.0005f) shown_ = target;
    }
    if (bar.mode == BarMode::halted) return {BarLookKind::halted, 0.0f, shown_, false};
    const bool held = held_ms_ >= static_cast<unsigned>(kStallMs) && shown_ > 0.0f && shown_ == target;
    return {BarLookKind::filled, phase, shown_, motion && held};
  }

 private:
  float shown_ = 0.0f, target_ = 0.0f;
  bool has_target_ = false;
  unsigned held_ms_ = 0, clock_ms_ = 0;
};

}  // namespace usque::setup::paint
