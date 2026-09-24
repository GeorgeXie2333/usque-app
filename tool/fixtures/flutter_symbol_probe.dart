// Standalone diagnostic fixture; never imported or bundled by the application.
@pragma('vm:never-inline')
void sizeSymbolProbe() => throw StateError('controlled size symbol probe');

void main() => sizeSymbolProbe();
