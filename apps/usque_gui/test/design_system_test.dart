import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:usque/core/usque_theme.dart';
import 'package:usque/models/app_models.dart';
import 'package:usque/widgets/animated_index_stack.dart';
import 'package:usque/widgets/common.dart';
import 'package:usque/widgets/connection_ring.dart';

/// Counts its own builds and holds a value, so a test can tell "still mounted"
/// apart from "rebuilt from scratch".
class _Counter extends StatefulWidget {
  const _Counter({required this.label, super.key});

  final String label;

  @override
  State<_Counter> createState() => _CounterState();
}

class _CounterState extends State<_Counter> {
  int taps = 0;

  @override
  Widget build(BuildContext context) {
    return TextButton(
      onPressed: () => setState(() => taps += 1),
      child: Text('${widget.label}:$taps'),
    );
  }
}

/// Reports whether its tickers are running at the moment it builds.
class _TickerProbe extends StatelessWidget {
  const _TickerProbe({required this.label});

  final String label;

  @override
  Widget build(BuildContext context) {
    return Text('$label:${TickerMode.valuesOf(context).enabled}');
  }
}

Widget _host(Widget child) {
  return MaterialApp(
    theme: UsqueTheme.light(),
    home: Scaffold(body: child),
  );
}

void main() {
  group('AnimatedIndexStack', () {
    testWidgets('keeps hidden sections mounted with their state', (
      tester,
    ) async {
      int index = 0;
      await tester.pumpWidget(
        _host(
          StatefulBuilder(
            builder: (context, setState) => Column(
              children: <Widget>[
                TextButton(
                  onPressed: () => setState(() => index = index == 0 ? 1 : 0),
                  child: const Text('switch'),
                ),
                Expanded(
                  child: AnimatedIndexStack(
                    index: index,
                    children: const <Widget>[
                      _Counter(label: 'first', key: ValueKey<String>('first')),
                      _Counter(label: 'second'),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      );

      await tester.tap(find.text('first:0'));
      await tester.pumpAndSettle();
      expect(find.text('first:1'), findsOneWidget);

      await tester.tap(find.text('switch'));
      await tester.pumpAndSettle();

      // The hidden section is still in the tree, carrying its own state.
      expect(find.text('first:1', skipOffstage: false), findsOneWidget);
      expect(find.text('first:1'), findsNothing);
      expect(find.text('second:0'), findsOneWidget);

      await tester.tap(find.text('switch'));
      await tester.pumpAndSettle();
      expect(find.text('first:1'), findsOneWidget);
    });

    testWidgets('hidden sections do not take taps or run tickers', (
      tester,
    ) async {
      await tester.pumpWidget(
        _host(
          const AnimatedIndexStack(
            index: 0,
            children: <Widget>[
              _TickerProbe(label: 'front'),
              _TickerProbe(label: 'back'),
            ],
          ),
        ),
      );

      expect(find.text('front:true'), findsOneWidget);
      expect(find.text('back:false', skipOffstage: false), findsOneWidget);
    });
  });

  group('ConnectionRing', () {
    for (final ConnectionPhase phase in ConnectionPhase.values) {
      testWidgets('builds for $phase', (tester) async {
        await tester.pumpWidget(
          _host(
            ConnectionRing(
              phase: phase,
              busy: false,
              actionLabel: 'Connect',
              onPressed: () {},
              semanticLabel: 'ring',
            ),
          ),
        );
        await tester.pump(const Duration(milliseconds: 800));
        expect(find.byType(ConnectionRing), findsOneWidget);
        expect(find.text('Connect'), findsOneWidget);
        expect(tester.takeException(), isNull);
      });
    }

    testWidgets('settles once the tunnel is up', (tester) async {
      await tester.pumpWidget(
        _host(
          const ConnectionRing(
            phase: ConnectionPhase.connectingH3,
            busy: false,
            actionLabel: 'Connect',
            onPressed: null,
          ),
        ),
      );
      // Scanning repeats, so the frame scheduler never goes quiet here.
      expect(tester.hasRunningAnimations, isTrue);

      await tester.pumpWidget(
        _host(
          const ConnectionRing(
            phase: ConnectionPhase.connected,
            busy: false,
            actionLabel: 'Disconnect',
            onPressed: null,
          ),
        ),
      );
      // The lock-in is finite: pumpAndSettle would hang on a repeating one.
      await tester.pumpAndSettle();
      expect(tester.hasRunningAnimations, isFalse);
    });

    testWidgets('skips the sweep under reduced motion', (tester) async {
      await tester.pumpWidget(
        MaterialApp(
          theme: UsqueTheme.light(),
          home: const MediaQuery(
            data: MediaQueryData(disableAnimations: true),
            child: Scaffold(
              body: ConnectionRing(
                phase: ConnectionPhase.connectingH3,
                busy: false,
                actionLabel: 'Connect',
                onPressed: null,
              ),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(tester.hasRunningAnimations, isFalse);
    });

    test('maps every phase to a ring mode', () {
      expect(RingState.of(ConnectionPhase.disconnected).mode, RingMode.idle);
      expect(RingState.of(ConnectionPhase.connectingH2).mode, RingMode.scan);
      expect(RingState.of(ConnectionPhase.connected).mode, RingMode.steady);
      expect(RingState.of(ConnectionPhase.degraded).tone, RingTone.caution);
      expect(RingState.of(ConnectionPhase.error).mode, RingMode.fault);
    });
  });

  group('PanelStack', () {
    testWidgets('spaces every neighbouring panel the same', (tester) async {
      await tester.pumpWidget(
        _host(
          const PanelStack(
            children: <Widget>[
              SizedBox(height: 40, child: Text('a')),
              SizedBox(height: 40, child: Text('b')),
              SizedBox(height: 40, child: Text('c')),
            ],
          ),
        ),
      );

      final double firstGap =
          tester.getTopLeft(find.text('b')).dy -
          tester.getBottomLeft(find.text('a')).dy;
      final double secondGap =
          tester.getTopLeft(find.text('c')).dy -
          tester.getBottomLeft(find.text('b')).dy;
      expect(firstGap, secondGap);
      expect(firstGap, greaterThan(0));
    });
  });
}
