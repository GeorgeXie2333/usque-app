import 'package:flutter/material.dart';
import 'package:lucide_icons_flutter/lucide_icons.dart';

import '../core/legal_links.dart';
import '../state/app_controller.dart';
import '../widgets/clear_data_panel.dart';
import '../widgets/common.dart';
import '../widgets/external_link.dart';

/// Version, the non-affiliation notice and the documents that stay relevant
/// after first launch: Cloudflare's terms and privacy policies, the source
/// and the bundled open-source licenses. Rows use the Settings list metrics.
class AboutScreen extends StatelessWidget {
  const AboutScreen({required this.controller, super.key});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final strings = controller.strings;
    final material = MaterialLocalizations.of(context);
    void open(String url) => openExternalLink(context, strings, url);
    return SubPage(
      contentWidth: 880,
      title: material.aboutListTileTitle('Usque'),
      subtitle: strings.get('app_version'),
      backLabel: strings.get('back'),
      child: PanelStack(
        spacing: 24,
        children: <Widget>[
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                Text(strings.get('unofficial')),
                const SizedBox(height: 8),
                HintText(strings.get('trademark_attribution')),
              ],
            ),
          ),
          ContentList(
            children: <Widget>[
              for (final (index, (labelKey, url))
                  in kCloudflareLegalLinks.indexed)
                LinkRow(
                  key: ValueKey('about-$labelKey'),
                  icon: const <IconData>[
                    LucideIcons.fileText,
                    LucideIcons.shieldCheck,
                    LucideIcons.building2,
                  ][index],
                  title: strings.get(labelKey),
                  onTap: () => open(url),
                ),
              LinkRow(
                key: const ValueKey('about-source-code'),
                icon: LucideIcons.code2,
                title: strings.get('source_code'),
                onTap: () => open(kSourceCodeUrl),
              ),
              LinkRow(
                key: const ValueKey('about-licenses'),
                icon: LucideIcons.scrollText,
                title: material.licensesPageTitle,
                onTap: () =>
                    showLicensePage(context: context, applicationName: 'Usque'),
              ),
            ],
          ),
          // Shares the notice's text edge above the links.
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8),
            child: ExpansionTile(
              key: const PageStorageKey<String>('app-technical-details'),
              title: Text(strings.get('technical_details')),
              tilePadding: EdgeInsets.zero,
              childrenPadding: const EdgeInsets.only(bottom: 16),
              expandedCrossAxisAlignment: CrossAxisAlignment.stretch,
              children: const <Widget>[
                SelectableText(
                  'IPC API: usque.v1',
                  key: PageStorageKey<String>('app-ipc-api'),
                ),
              ],
            ),
          ),
          // The application's only destructive action, kept apart from the
          // links above in its own danger panel.
          ClearDataPanel(controller: controller),
        ],
      ),
    );
  }
}
