import 'dart:math' as math;

import 'package:flutter/rendering.dart';
import 'package:flutter/widgets.dart';

/// A row of form actions that stacks them at full width when they do not fit.
///
/// In a row it matches `OverflowBar`: with [leading], the first child (Back)
/// keeps the start edge and the rest spread to the end; otherwise every child
/// sits at the end. Stacked, each child spans the width in order of emphasis:
/// the primary action on top and the leading child last, so a long label
/// never leaves Back floating above it.
class StackingActionBar extends MultiChildRenderObjectWidget {
  const StackingActionBar({
    required this.leading,
    required super.children,
    this.spacing = 12,
    super.key,
  });

  /// Whether the first child is a backward action.
  final bool leading;

  /// The gap between children in both arrangements.
  final double spacing;

  @override
  RenderStackingActionBar createRenderObject(BuildContext context) =>
      RenderStackingActionBar(
        leading: leading,
        spacing: spacing,
        textDirection: Directionality.of(context),
      );

  @override
  void updateRenderObject(
    BuildContext context,
    RenderStackingActionBar renderObject,
  ) {
    renderObject
      ..leading = leading
      ..spacing = spacing
      ..textDirection = Directionality.of(context);
  }
}

class _StackingActionBarParentData extends ContainerBoxParentData<RenderBox> {}

class RenderStackingActionBar extends RenderBox
    with
        ContainerRenderObjectMixin<RenderBox, _StackingActionBarParentData>,
        RenderBoxContainerDefaultsMixin<
          RenderBox,
          _StackingActionBarParentData
        > {
  RenderStackingActionBar({
    required this._leading,
    required this._spacing,
    required this._textDirection,
  });

  bool _leading;
  set leading(bool value) {
    if (value == _leading) return;
    _leading = value;
    markNeedsLayout();
  }

  double _spacing;
  set spacing(double value) {
    if (value == _spacing) return;
    _spacing = value;
    markNeedsLayout();
  }

  TextDirection _textDirection;
  set textDirection(TextDirection value) {
    if (value == _textDirection) return;
    _textDirection = value;
    markNeedsLayout();
  }

  /// Whether the children are currently stacked; exposed for tests.
  bool get stacked => _stacked;
  bool _stacked = false;

  @override
  void setupParentData(RenderBox child) {
    if (child.parentData is! _StackingActionBarParentData) {
      child.parentData = _StackingActionBarParentData();
    }
  }

  List<RenderBox> get _children => <RenderBox>[
    for (
      RenderBox? child = firstChild;
      child != null;
      child = childAfter(child)
    )
      child,
  ];

  /// Top to bottom when stacked: the leading child moves last.
  List<RenderBox> get _stackOrder {
    final children = _children;
    return _leading && children.length > 1
        ? <RenderBox>[...children.skip(1), children.first]
        : children;
  }

  double _gaps(int count) => _spacing * math.max(0, count - 1);

  bool _fits(Iterable<double> widths, double maxWidth) =>
      widths.fold<double>(0, (sum, width) => sum + width) +
          _gaps(widths.length) <=
      maxWidth;

  @override
  double computeMinIntrinsicWidth(double height) => _children.fold<double>(
    0,
    (width, child) => math.max(width, child.getMinIntrinsicWidth(height)),
  );

  @override
  double computeMaxIntrinsicWidth(double height) =>
      _children.fold<double>(
        0,
        (width, child) => width + child.getMaxIntrinsicWidth(height),
      ) +
      _gaps(childCount);

  @override
  double computeMinIntrinsicHeight(double width) => _intrinsicHeight(width);

  @override
  double computeMaxIntrinsicHeight(double width) => _intrinsicHeight(width);

  double _intrinsicHeight(double width) {
    final children = _children;
    final row = children.map(
      (child) => child.getMaxIntrinsicWidth(double.infinity),
    );
    if (_fits(row, width)) {
      return children.fold<double>(
        0,
        (height, child) => math.max(
          height,
          child.getMaxIntrinsicHeight(child.getMaxIntrinsicWidth(width)),
        ),
      );
    }
    return children.fold<double>(
          0,
          (height, child) => height + child.getMaxIntrinsicHeight(width),
        ) +
        _gaps(children.length);
  }

  @override
  Size computeDryLayout(covariant BoxConstraints constraints) {
    final children = _children;
    if (children.isEmpty) return constraints.smallest;
    final loose = constraints.loosen();
    final sizes = <Size>[
      for (final child in children) child.getDryLayout(loose),
    ];
    if (_fits(sizes.map((size) => size.width), constraints.maxWidth)) {
      return constraints.constrain(
        Size(
          constraints.maxWidth,
          sizes.fold<double>(
            0,
            (height, size) => math.max(height, size.height),
          ),
        ),
      );
    }
    final tight = BoxConstraints.tightFor(width: constraints.maxWidth);
    return constraints.constrain(
      Size(
        constraints.maxWidth,
        children.fold<double>(
              0,
              (height, child) => height + child.getDryLayout(tight).height,
            ) +
            _gaps(children.length),
      ),
    );
  }

  @override
  void performLayout() {
    final children = _children;
    if (children.isEmpty) {
      size = constraints.smallest;
      return;
    }
    final loose = constraints.loosen();
    for (final child in children) {
      child.layout(loose, parentUsesSize: true);
    }
    final double maxWidth = constraints.maxWidth;
    _stacked = !_fits(children.map((child) => child.size.width), maxWidth);
    if (_stacked) {
      final tight = BoxConstraints.tightFor(width: maxWidth);
      double y = 0;
      for (final child in _stackOrder) {
        child.layout(tight, parentUsesSize: true);
        (child.parentData! as _StackingActionBarParentData).offset = Offset(
          0,
          y,
        );
        y += child.size.height + _spacing;
      }
      size = constraints.constrain(Size(maxWidth, y - _spacing));
      return;
    }

    final double rowHeight = children.fold<double>(
      0,
      (height, child) => math.max(height, child.size.height),
    );
    size = constraints.constrain(Size(maxWidth, rowHeight));
    final double childrenWidth = children.fold<double>(
      0,
      (width, child) => width + child.size.width,
    );
    final bool spread = _leading && children.length > 1;
    final double gap = spread
        ? (size.width - childrenWidth) / (children.length - 1)
        : _spacing;
    double x = spread ? 0 : size.width - childrenWidth - _gaps(children.length);
    final bool rtl = _textDirection == TextDirection.rtl;
    for (final child in children) {
      (child.parentData! as _StackingActionBarParentData).offset = Offset(
        rtl ? size.width - x - child.size.width : x,
        (rowHeight - child.size.height) / 2,
      );
      x += child.size.width + gap;
    }
  }

  @override
  bool hitTestChildren(BoxHitTestResult result, {required Offset position}) =>
      defaultHitTestChildren(result, position: position);

  @override
  void paint(PaintingContext context, Offset offset) =>
      defaultPaint(context, offset);
}
