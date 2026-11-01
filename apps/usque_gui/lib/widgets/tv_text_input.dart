import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../core/app_strings.dart';
import '../models/app_models.dart';
import 'desktop_shortcuts.dart';
import 'tv_input_scope.dart';

/// Adapts native Material inputs without owning their drafts or validation.
class TvTextInput extends StatefulWidget {
  const TvTextInput({
    required this.builder,
    this.focusNode,
    this.enabled = true,
    this.readOnly = false,
    this.textInputAction,
    this.onTap,
    this.onEditingComplete,
    super.key,
  });
  final Widget Function(TvTextInputBinding input) builder;
  final FocusNode? focusNode;
  final bool enabled, readOnly;
  final TextInputAction? textInputAction;
  final GestureTapCallback? onTap;
  final VoidCallback? onEditingComplete;
  @override
  State<TvTextInput> createState() => _TvTextInputState();
}

class TvTextInputBinding {
  const TvTextInputBinding({
    required this.focusNode,
    required this.readOnly,
    required this.browsing,
    required this.onTap,
    required this.onEditingComplete,
  });
  final FocusNode? focusNode;
  final bool readOnly, browsing;
  final GestureTapCallback? onTap;
  final VoidCallback? onEditingComplete;
}

class _ActivateInputIntent extends Intent {
  const _ActivateInputIntent();
}

class _NavigateInputIntent extends Intent {
  const _NavigateInputIntent(this.direction);
  final TraversalDirection direction;
}

class _NavigateInputAction extends Action<_NavigateInputIntent> {
  _NavigateInputAction({required this.enabled, required this.onNavigate});
  final bool Function() enabled;
  final ValueChanged<TraversalDirection> onNavigate;

  @override
  bool isEnabled(_NavigateInputIntent intent) => enabled();

  @override
  Object? invoke(_NavigateInputIntent intent) {
    onNavigate(intent.direction);
    return null;
  }
}

class _TvTextInputState extends State<TvTextInput> {
  final _ownedFocus = FocusNode(debugLabel: 'TV text input');
  FocusNode get _focus => widget.focusNode ?? _ownedFocus;
  TvInputController? _controller;
  TvInputController? _localController;
  bool _tv = false, _editing = false, _starting = false, _disposing = false;
  LogicalKeyboardKey? _heldActivation;
  static final _activationKeys = <LogicalKeyboardKey>{
    LogicalKeyboardKey.select,
    LogicalKeyboardKey.enter,
    LogicalKeyboardKey.numpadEnter,
    LogicalKeyboardKey.gameButtonA,
  };
  bool get _canEdit => widget.enabled && !widget.readOnly;

  @override
  void initState() {
    super.initState();
    _focus.addListener(_focusChanged);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _tv = TvInputScope.enabled(context);
    if (_tv) {
      _controller =
          TvInputScope.maybeOf(context) ??
          (_localController ??= TvInputController(isTv: true));
    }
    // Depend on route currency as well as MediaQuery, including enclosing
    // navigators. A covered editor must never consume the new dialog's Back.
    if ((_editing || _starting) && (!_tv || !routeChainIsCurrent(context))) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && (!_tv || !routeChainIsCurrent(context))) {
          _controller?.finish(owner: this, restoreFocus: false);
        }
      });
    }
  }

  @override
  void didUpdateWidget(TvTextInput oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.focusNode != widget.focusNode) {
      (oldWidget.focusNode ?? _ownedFocus).removeListener(_focusChanged);
      _focus.addListener(_focusChanged);
    }
    if (!_canEdit && (_editing || _starting)) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted && !_canEdit) {
          _controller?.finish(owner: this, restoreFocus: false);
        }
      });
    }
  }

  void _focusChanged() {
    if (_disposing) return;
    if (!_focus.hasFocus && (_editing || _starting)) {
      _controller?.finish(owner: this, restoreFocus: false);
    }
  }

  Future<void> _activate() async {
    if (!_tv ||
        !_canEdit ||
        _editing ||
        _starting ||
        !routeChainIsCurrent(context)) {
      return;
    }
    _heldActivation = HardwareKeyboard.instance.logicalKeysPressed
        .where(_activationKeys.contains)
        .firstOrNull;
    _focus.requestFocus();
    setState(() => _starting = true);
    final started = await _controller!.begin(
      owner: this,
      context: context,
      onEnd: _ended,
    );
    if (!mounted || !started || !_canEdit || !_controller!.owns(this)) return;
    setState(() {
      _starting = false;
      _editing = true;
    });
  }

  void _ended(bool restoreFocus) {
    if (_disposing || !mounted) return;
    setState(() {
      _editing = false;
      _starting = false;
    });
    unawaited(SystemChannels.textInput.invokeMethod<void>('TextInput.hide'));
    if (restoreFocus) {
      final controller = _controller!;
      final revision = controller.focusRevision;
      final focus = _focus;
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (!mounted || _controller != controller) return;
        // A sibling can have requested focus earlier in this same frame.
        // Resolve it before deciding whether the old restoration is current.
        FocusManager.instance.applyFocusChangesIfNeeded();
        if (mounted &&
            _tv &&
            !_editing &&
            !_starting &&
            _focus == focus &&
            widget.enabled &&
            controller.canRestoreFocus(revision) &&
            routeChainIsCurrent(context)) {
          if (!focus.hasFocus) focus.requestFocus();
        }
      });
    }
  }

  void _complete() {
    // Submission callbacks may focus another field or replace the route. Do
    // not schedule a focus restoration that steals their chosen destination.
    _controller?.finish(owner: this, restoreFocus: false);
    if (widget.onEditingComplete case final complete?) {
      complete();
    } else if (widget.textInputAction == TextInputAction.next) {
      _focus.nextFocus();
    } else if (widget.textInputAction == TextInputAction.previous) {
      _focus.previousFocus();
    }
  }

  KeyEventResult _key(FocusNode node, KeyEvent event) {
    if (event.logicalKey == _heldActivation && event is KeyUpEvent) {
      setState(() => _heldActivation = null);
      return KeyEventResult.handled;
    }
    if ((_editing || _starting) &&
        event is KeyDownEvent &&
        (event.logicalKey == LogicalKeyboardKey.escape ||
            event.logicalKey == LogicalKeyboardKey.goBack)) {
      _controller?.consumeBack(context);
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  void dispose() {
    _disposing = true;
    _focus.removeListener(_focusChanged);
    _controller?.finish(owner: this, restoreFocus: false);
    _localController?.dispose();
    _ownedFocus.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final browsing = _tv && (!_editing || !_canEdit);
    final field = widget.builder(
      TvTextInputBinding(
        focusNode: _tv ? _focus : widget.focusNode,
        readOnly: widget.readOnly || browsing,
        browsing: browsing,
        onTap: _tv
            ? () {
                unawaited(_activate());
                widget.onTap?.call();
              }
            : widget.onTap,
        onEditingComplete: _tv ? _complete : widget.onEditingComplete,
      ),
    );
    if (!_tv) return field;
    return Focus(
      canRequestFocus: false,
      skipTraversal: true,
      onKeyEvent: _key,
      child: Shortcuts(
        shortcuts: <ShortcutActivator, Intent>{
          const SingleActivator(LogicalKeyboardKey.arrowUp):
              const _NavigateInputIntent(TraversalDirection.up),
          const SingleActivator(LogicalKeyboardKey.arrowDown):
              const _NavigateInputIntent(TraversalDirection.down),
          const SingleActivator(LogicalKeyboardKey.arrowLeft):
              const _NavigateInputIntent(TraversalDirection.left),
          const SingleActivator(LogicalKeyboardKey.arrowRight):
              const _NavigateInputIntent(TraversalDirection.right),
          for (final key in _activationKeys)
            if (browsing || key == _heldActivation)
              SingleActivator(key): const _ActivateInputIntent(),
        },
        child: Actions(
          actions: <Type, Action<Intent>>{
            _NavigateInputIntent: _NavigateInputAction(
              // A native dismissal can arrive before the next widget build.
              // Disabled actions fall through to normal text-editing shortcuts.
              enabled: () => !_disposing && (!_editing || !_canEdit),
              onNavigate: (direction) {
                _controller?.noteFocusNavigation();
                FocusManager.instance.primaryFocus?.focusInDirection(direction);
              },
            ),
            _ActivateInputIntent: CallbackAction<_ActivateInputIntent>(
              onInvoke: (_) {
                unawaited(_activate());
                return null;
              },
            ),
          },
          child: Semantics(
            readOnly: widget.readOnly,
            onTapHint: browsing && _canEdit
                ? AppStrings(
                    LocalePreference.system,
                    systemLocale: Localizations.localeOf(context),
                  ).get('edit')
                : null,
            onTap: _canEdit ? () => unawaited(_activate()) : null,
            child: field,
          ),
        ),
      ),
    );
  }
}

/// Material constructors below keep their original keys on the actual fields,
/// so FormField validation and existing widget integrations retain their API.
Widget tvTextField({
  Key? key,
  TextEditingController? controller,
  FocusNode? focusNode,
  InputDecoration? decoration = const InputDecoration(),
  TextInputType? keyboardType,
  TextInputAction? textInputAction,
  TextStyle? style,
  TextDirection? textDirection,
  bool autofocus = false,
  bool obscureText = false,
  bool autocorrect = true,
  bool enableSuggestions = true,
  int? maxLines = 1,
  int? minLines,
  int? maxLength,
  ValueChanged<String>? onChanged,
  ValueChanged<String>? onSubmitted,
  VoidCallback? onEditingComplete,
  List<TextInputFormatter>? inputFormatters,
  bool? enabled,
  bool readOnly = false,
  Iterable<String>? autofillHints = const <String>[],
  GestureTapCallback? onTap,
}) => TvTextInput(
  key: key == null ? null : ValueKey(key),
  focusNode: focusNode,
  enabled: enabled ?? true,
  readOnly: readOnly,
  textInputAction: textInputAction,
  onTap: onTap,
  onEditingComplete: onEditingComplete,
  builder: (input) => TextField(
    key: key,
    controller: controller,
    focusNode: input.focusNode,
    decoration: decoration,
    keyboardType: keyboardType,
    textInputAction: textInputAction,
    style: style,
    textDirection: textDirection,
    autofocus: autofocus,
    obscureText: obscureText,
    autocorrect: autocorrect,
    enableSuggestions: enableSuggestions,
    maxLines: maxLines,
    minLines: minLines,
    maxLength: maxLength,
    onChanged: onChanged,
    onSubmitted: onSubmitted,
    onEditingComplete: input.onEditingComplete,
    inputFormatters: inputFormatters,
    enabled: enabled,
    readOnly: input.readOnly,
    autofillHints: autofillHints,
    onTap: input.onTap,
    showCursor: input.browsing ? false : null,
  ),
);

Widget tvTextFormField({
  Key? key,
  TextEditingController? controller,
  String? initialValue,
  FocusNode? focusNode,
  InputDecoration? decoration = const InputDecoration(),
  TextInputType? keyboardType,
  TextInputAction? textInputAction,
  TextStyle? style,
  TextDirection? textDirection,
  bool autofocus = false,
  bool obscureText = false,
  bool autocorrect = true,
  bool enableSuggestions = true,
  int? maxLines = 1,
  int? minLines,
  int? maxLength,
  ValueChanged<String>? onChanged,
  ValueChanged<String>? onFieldSubmitted,
  VoidCallback? onEditingComplete,
  List<TextInputFormatter>? inputFormatters,
  bool? enabled,
  bool readOnly = false,
  FormFieldValidator<String>? validator,
  GestureTapCallback? onTap,
}) => TvTextInput(
  key: key == null ? null : ValueKey(key),
  focusNode: focusNode,
  enabled: enabled ?? true,
  readOnly: readOnly,
  textInputAction: textInputAction,
  onTap: onTap,
  onEditingComplete: onEditingComplete,
  builder: (input) => TextFormField(
    key: key,
    controller: controller,
    initialValue: initialValue,
    focusNode: input.focusNode,
    decoration: decoration,
    keyboardType: keyboardType,
    textInputAction: textInputAction,
    style: style,
    textDirection: textDirection,
    autofocus: autofocus,
    obscureText: obscureText,
    autocorrect: autocorrect,
    enableSuggestions: enableSuggestions,
    maxLines: maxLines,
    minLines: minLines,
    maxLength: maxLength,
    onChanged: onChanged,
    onFieldSubmitted: onFieldSubmitted,
    onEditingComplete: input.onEditingComplete,
    inputFormatters: inputFormatters,
    enabled: enabled,
    readOnly: input.readOnly,
    validator: validator,
    onTap: input.onTap,
    showCursor: input.browsing ? false : null,
  ),
);
