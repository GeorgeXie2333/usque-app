/// Supplemental feature strings for Vietnamese.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowVi = <String, String>{
  'local_proxy_settings': 'Cài đặt proxy cục bộ',
  'proxy_switches_hint':
      'Thay đổi công tắc được lưu tự động. Dùng Áp dụng thay đổi để lưu chỉnh sửa địa chỉ lắng nghe và DNS.',
  'cc_label': 'Kiểm soát tắc nghẽn HTTP/3',
  'cc_help': 'Có hiệu lực ở lần kết nối thủ công tiếp theo.',
  'cc_upgrade': 'Cập nhật Usque trong Cài đặt.',
  'cc_h2': 'Tùy chọn này chỉ ảnh hưởng đến kết nối HTTP/3.',
  'cc_saved': 'Đã lưu',
  'cc_pending': 'Chờ lần kết nối thủ công tiếp theo.',
  'save_changes': 'Áp dụng thay đổi',
  'saving_changes': 'Đang áp dụng thay đổi…',
  'unsaved_changes': 'Thay đổi chưa áp dụng',
  'changes_applied': 'Đã áp dụng thay đổi',
  'changes_apply_hint': 'Chỉnh sửa chỉ có hiệu lực sau khi bạn áp dụng.',
  'changes_failed':
      'Không thể áp dụng thay đổi. Hãy xem lại giá trị đã lưu rồi thử lại.',
  'form_errors': 'Kiểm tra các trường được tô sáng trước khi áp dụng thay đổi.',
  'discard_changes_title': 'Bỏ các thay đổi chưa áp dụng?',
  'discard_changes_body':
      'Chỉnh sửa của bạn chưa được áp dụng. Tiếp tục sửa để lưu, hoặc bỏ chúng để rời đi.',
  'keep_editing': 'Tiếp tục chỉnh sửa',
  'discard_changes': 'Bỏ thay đổi',
  'invalid_port': 'Nhập cổng từ 1 đến 65535.',
  'listener_exposure': 'Địa chỉ trình lắng nghe cho phép truy cập LAN',
  'invalid_ipv4': 'Nhập địa chỉ IPv4 hợp lệ, ví dụ 127.0.0.1.',
  'invalid_ipv6': 'Nhập địa chỉ IPv6 hợp lệ, ví dụ ::1.',
  'output_running': 'Đang chạy',
  'output_waiting': 'Đã bật · chưa chạy',
  'output_disabled': 'Đã tắt',
  'output_starting': 'Đang khởi động',
  'output_stopping': 'Đang dừng',
  'output_reconnecting': 'Đang kết nối lại',
  'output_degraded': 'Hạn chế',
  'output_error': 'Lỗi',
  'output_unknown': 'Không có trạng thái',
  'shared_network_scope': 'Cài đặt mạng được dùng chung cho mọi tài khoản.',
  'connection_details': 'Chi tiết kết nối',
  'home_overview': 'Tổng quan kết nối',
  'home_exit_region': 'Vùng thoát',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Lưu lượng',
  'home_traffic_window': '60 giây gần nhất',
  'home_traffic_idle': 'Lưu lượng sẽ hiển thị sau khi kết nối',
  'home_traffic_waiting': 'Đang chờ dữ liệu lưu lượng',
  'home_traffic_unavailable': 'Chưa có lịch sử lưu lượng',
  'home_traffic_stale': 'Cập nhật lưu lượng bị chậm',
  'home_outputs_next': 'Có thể dùng sau khi kết nối',
  'home_outputs_retry': 'VPN và proxy cho lần kết nối tiếp theo',
  'connection_protection_group': 'Kết nối & bảo vệ',
  'proxy_routing_group': 'Proxy & định tuyến',
  'application_group': 'Ứng dụng',
  'proxy_settings_link': 'Địa chỉ trình lắng nghe, cổng, xác thực và DNS.',
  'proxy_auth_separate':
      'Dùng nút “Lưu tên người dùng và mật khẩu” bên dưới để áp dụng các thay đổi này.',
  'reset_draft_hint':
      'Giá trị mặc định sẽ được nạp vào biểu mẫu này. Áp dụng thay đổi để chúng có hiệu lực.',
};

const Map<String, String> kNetworkQualityVi = <String, String>{
  'nq_range': 'Phạm vi',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Thời gian khứ hồi',
  'diag_check_quality_packet_loss': 'Mất gói',
  'diag_check_quality_queue_pressure': 'Áp lực hàng đợi',
  'diag_check_quality_pmtu': 'MTU đường dẫn',
  'diag_check_transport_migration_capability': 'Chuyển họ địa chỉ cùng loại',
  'diag_check_dns_direct_encrypted_configuration': 'Cấu hình DNS trực tiếp',
  'diag_check_dns_direct_encrypted_runtime_state':
      'Trạng thái chạy DNS trực tiếp',
  'diag_check_dns_direct_encrypted_reachability': 'Khả năng tới DNS mã hóa',
  'diag_check_transport_h3_path_validation_probe': 'Bắt tay QUIC độc lập',
  'nq_finding_unavailable': 'Phép đo này không có trong trạng thái hiện tại.',
  'nq_finding_invalid_configuration': 'Cấu hình DNS tùy chỉnh không hợp lệ.',
  'nq_finding_dns_system':
      'Đang dùng DNS của mạng hiện tại. Kiểm tra DNS mã hóa không áp dụng trong trường hợp này.',
  'nq_finding_unsupported':
      'Cập nhật Usque để dùng DNS mã hóa. Ứng dụng không tự chuyển sang DNS không mã hóa.',
  'nq_finding_dns_custom_valid':
      'Cấu hình DNS mã hóa tùy chỉnh hợp lệ. Đã tắt dự phòng DNS không mã hóa (plaintext).',
  'nq_finding_stale': 'Số liệu đã cũ hoặc mạng vật lý đã đổi.',
  'nq_finding_rtt_high': 'Thời gian khứ hồi đo được đang cao.',
  'nq_finding_healthy': 'Các chỉ số kết nối đo được đều bình thường.',
  'nq_finding_loss_high': 'Mất gói trong khoảng đo đang cao.',
  'nq_finding_queue_pressure':
      'Trong kết nối này có dữ liệu đang chờ gửi hoặc đã bị loại bỏ.',
  'nq_finding_pmtu_degraded':
      'Không thể xác nhận kích thước gói tin phù hợp với đường truyền.',
  'nq_finding_migration_reconnect': 'Cần kết nối lại sau khi chuyển mạng.',
  'nq_finding_dns_changed': 'Chế độ DNS đã lưu khác với kết nối đang chạy.',
  'nq_finding_dns_runtime': 'DNS mã hóa đang hoạt động.',
  'nq_finding_dns_degraded':
      'DNS mã hóa gặp sự cố. Các truy vấn thất bại không được gửi đến DNS không mã hóa của mạng hiện tại.',
  'nq_finding_probe_unsafe': 'Phép đo này không có trong trạng thái hiện tại.',
  'nq_finding_probe_success': 'Kiểm tra này đã đạt.',
  'nq_finding_probe_cancelled': 'Kiểm tra này đã bị hủy.',
  'nq_finding_probe_timeout': 'Kiểm tra chẩn đoán đã hết thời gian chờ',
  'nq_finding_probe_failed': 'Kiểm tra này thất bại.',
  'diag_fix_nq_profile':
      'Xem lại các trường DNS tùy chỉnh và tên chứng chỉ. Đừng tắt xác minh TLS.',
  'diag_fix_nq_retry': 'Đợi mạng ổn định, rồi thử lại.',
  'diag_fix_nq_network':
      'Kiểm tra kết nối cục bộ và so sánh mẫu mới trước khi đổi cài đặt.',
  'diag_fix_nq_reconnect': 'Kết nối lại để áp dụng cấu hình đã lưu.',
  'nav_network_quality': 'Chất lượng',
  'network_quality': 'Chất lượng mạng',
  'nq_subtitle': 'Đọc kết nối, không chỉ tốc độ.',
  'nq_local_only': 'Chỉ đo cục bộ. Không tải gì lên.',
  'nq_doctor': 'Chạy Network Doctor',
  'nq_doctor_help':
      'Kiểm tra chuẩn chỉ đọc trạng thái cục bộ. Chúng không mở kết nối ngoài hay đổi cài đặt của bạn.',
  'nq_live': 'Trực tiếp',
  'nq_stale': 'Số liệu đã cũ',
  'nq_updated': 'Mẫu gần nhất',
  'nq_seconds': '{count} giây trước',
  'nq_good': 'Tốt',
  'nq_fair': 'Khá',
  'nq_poor': 'Kém',
  'nq_limited': 'Dữ liệu hạn chế',
  'nq_disconnected': 'Đã ngắt kết nối',
  'nq_connecting': 'Đang kết nối',
  'nq_connected': 'Đã kết nối',
  'nq_unavailable': 'Không có sẵn',
  'nq_not_ready': 'Chưa sẵn sàng',
  'nq_unsupported': 'Không được hỗ trợ',
  'nq_capability_missing':
      'Phiên bản này không thể hiển thị chất lượng kết nối. Bạn vẫn có thể kết nối và ngắt kết nối. Hãy cập nhật Usque trong Cài đặt.',
  'nq_empty': 'Kết nối để xem số liệu đo.',
  'nq_stale_help': 'Cập nhật đã tạm dừng. Đang hiển thị số liệu gần nhất.',
  'nq_rtt': 'Thời gian khứ hồi',
  'nq_latest': 'Mới nhất',
  'nq_smoothed': 'Đã làm mượt',
  'nq_minimum': 'Tối thiểu',
  'nq_h2_ping': 'PING giao thức HTTP/2',
  'nq_h3_rtt': 'Đo đường QUIC',
  'nq_throughput': 'Thông lượng',
  'nq_download': 'Tải xuống',
  'nq_upload': 'Tải lên',
  'nq_one_second': '1 giây',
  'nq_five_seconds': 'Trung bình 5 giây',
  'nq_loss': 'Mất gói',
  'nq_loss_h2': 'HTTP/2 không cho thấy mất gói tương đương.',
  'nq_loss_interval': 'Đo trên khoảng gần nhất; không phải mất gói cả đời.',
  'nq_congestion': 'Tắc nghẽn',
  'nq_cwnd': 'Cửa sổ tắc nghẽn',
  'nq_in_flight': 'Bytes đang gửi',
  'nq_send_rate': 'Tốc độ giao',
  'nq_h2_window': 'Cửa sổ nhận HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Kết nối',
  'nq_stalls': 'Khựng vì dung lượng',
  'nq_pmtu': 'MTU đường dẫn',
  'nq_outer_pmtu': 'Giới hạn tải UDP ngoài',
  'nq_inner_payload': 'Giới hạn tải CONNECT-IP',
  'nq_pmtu_help':
      'Kích thước gói tin mà đường truyền có thể chuyển được. Kiểm tra tự động giúp giảm mất gói và không tăng MTU của VPN đã đặt trong cài đặt mạng nâng cao.',
  'nq_migration': 'Chuyển mạng',
  'nq_migration_help':
      'Cố gắng giữ kết nối khi chuyển giữa Wi-Fi và dữ liệu di động. Hai mạng phải dùng cùng phiên bản IP, tức IPv4 hoặc IPv6. Mỗi lần chỉ dùng một đường truyền, không cộng gộp tốc độ của các mạng.',
  'nq_attempts': 'Lần thử',
  'nq_successes': 'Thành công',
  'nq_failures': 'Thất bại',
  'nq_last_duration': 'Thời lượng gần nhất',
  'nq_direct_dns': 'DNS trực tiếp',
  'nq_system_dns': 'DNS của mạng hiện tại',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Sẵn sàng',
  'nq_degraded': 'Suy giảm',
  'nq_timeouts': 'Hết thời gian',
  'nq_last_rtt': 'RTT gần nhất',
  'nq_dns_redacted':
      'Tên bộ phân giải và địa chỉ bootstrap chỉ hiện trong cài đặt.',
  'nq_queues': 'Áp lực hàng đợi',
  'nq_queue_details': 'Hàng đợi tầng thấp',
  'nq_queue_empty': 'Chưa có phép đo hàng đợi.',
  'nq_current_capacity': 'Hiện tại / dung lượng',
  'nq_high_water': 'Mốc cao nhất',
  'nq_drops': 'Gói bị loại',
  'nq_oldest': 'Mục cũ nhất',
  'nq_tunToTransport': 'Thiết bị → truyền tải',
  'nq_proxyToTransport': 'Proxy → truyền tải',
  'nq_transportOutgoing': 'Truyền tải đi',
  'nq_h3DatagramSend': 'Datagram QUIC',
  'nq_h3WireSend': 'Đầu ra UDP',
  'nq_transportToTun': 'Truyền tải → thiết bị',
  'nq_transportToProxy': 'Truyền tải → Proxy',
  'nq_directDns': 'Yêu cầu DNS trực tiếp',
  'nq_unknown_queue': 'Hàng đợi khác',
  'nq_trends': '60 giây gần nhất',
  'nq_samples': 'mẫu',
  'nq_pause': 'Tạm dừng biểu đồ',
  'nq_resume': 'Tiếp tục biểu đồ',
  'nq_paused': 'Biểu đồ đã tạm dừng',
  'nq_gaps': 'Mẫu thiếu được hiện thành khoảng trống.',
  'nq_phase_idle': 'Nhàn rỗi',
  'nq_phase_preparing_socket': 'Đang chuẩn bị đường',
  'nq_phase_probing': 'Đang dò',
  'nq_phase_validated': 'Đã xác thực',
  'nq_phase_promoting': 'Đang đổi đường',
  'nq_phase_stable': 'Ổn định',
  'nq_phase_aborted': 'Đã dừng',
  'nq_phase_revalidating': 'Đang xác thực lại',
  'nq_phase_degraded': 'Suy giảm',
  'nq_phase_unknown': 'Chưa sẵn sàng',
  'nq_phase_unsupported': 'Không được hỗ trợ',
  'nq_reason_family_unavailable':
      'Mạng mới không dùng được cùng phiên bản IP. Hãy kết nối lại.',
  'nq_reason_socket_protect_failed':
      'Không thể sử dụng mạng mới một cách an toàn. Nếu kết nối không tự khôi phục, hãy kết nối lại.',
  'nq_reason_generation_changed_during_setup':
      'Mạng lại thay đổi trong lúc thiết lập.',
  'nq_reason_peer_cid_unavailable':
      'Máy chủ không giữ được kết nối trên mạng mới. Kết nối lại nếu cần.',
  'nq_reason_local_cid_unavailable':
      'Usque không giữ được kết nối trên mạng mới. Kết nối lại nếu cần.',
  'nq_reason_path_probe_rejected':
      'Kiểm tra kết nối của mạng mới thất bại. Kiểm tra khả năng truy cập Internet.',
  'nq_reason_path_validation_timeout':
      'Mạng mới không phản hồi kịp thời. Kiểm tra khả năng truy cập Internet và kết nối lại nếu cần.',
  'nq_reason_superseded': 'Mạng lại thay đổi trước khi chuyển xong.',
  'nq_reason_promotion_failed':
      'Không thể hoàn tất chuyển mạng một cách an toàn. Nếu kết nối không khôi phục, hãy thử lại.',
  'nq_reason_connection_closed':
      'Kết nối đã đóng trong lúc chuyển mạng. Hãy kết nối lại.',
  'nq_reason_unsupported': 'Kết nối này không hỗ trợ chuyển đường.',
  'nq_reason_unknown': 'Không có lý do được hỗ trợ.',
  'nq_dns_custom': 'Bộ phân giải mã hóa tùy chỉnh',
  'nq_dns_server': 'Tên miền máy chủ DNS',
  'nq_dns_path': 'Đường HTTPS',
  'nq_dns_port': 'Cổng (0 dùng mặc định)',
  'nq_dns_bootstrap': 'Địa chỉ IP của máy chủ DNS',
  'nq_dns_bootstrap_help':
      'Nhập 1–8 địa chỉ IP do nhà cung cấp DNS cung cấp, mỗi địa chỉ một dòng. Ví dụ: 1.1.1.1. Các địa chỉ này cho phép kết nối trực tiếp mà không cần tra cứu tên máy chủ trước.',
  'nq_dns_no_fallback':
      'Nếu DNS trực tiếp mã hóa thất bại, truy vấn thất bại. Không bao giờ quay về DNS hệ thống hoặc DNS không mã hóa (plaintext).',
  'nq_dns_system_privacy':
      'Nhà cung cấp DNS của mạng hiện tại có thể thấy các tên miền được truy vấn cho lưu lượng trực tiếp.',
  'nq_dns_scope':
      'Chỉ ảnh hưởng đến lưu lượng khớp quy tắc kết nối trực tiếp theo quốc gia hoặc khu vực. DNS của lưu lượng VPN không thay đổi.',
  'nq_dns_no_capability':
      'Cập nhật Usque để dùng DNS mã hóa cho kết nối trực tiếp. Các cài đặt đã lưu vẫn được giữ lại. Nếu chấp nhận ảnh hưởng đến quyền riêng tư, bạn có thể tự chọn “DNS của mạng hiện tại”.',
  'nq_dns_invalid_name':
      'Nhập tên miền như dns.example.com, không kèm https://, cổng hay khoảng trắng.',
  'nq_dns_invalid_path':
      'Nhập đường dẫn như /dns-query, tối đa 256 ký tự, không có khoảng trắng hoặc phần chứa ? hay #.',
  'nq_dns_invalid_bootstrap': 'Nhập 1–8 địa chỉ IP cho máy chủ DNS.',
  'nq_dns_invalid_port':
      'Nhập cổng từ 1 đến 65535, hoặc 0 để dùng cổng mặc định.',
  'nq_dns_invalid_mode': 'Chọn chế độ DNS được hỗ trợ.',
  'nq_doctor_deep_title': 'Chạy kiểm tra mạng sâu?',
  'nq_doctor_deep_body':
      'Các kiểm tra có thể gửi lưu lượng thử nghiệm. Thời gian tối đa là 15 giây và có thể hủy. Cài đặt kết nối của bạn sẽ không thay đổi.',
  'nq_doctor_deep_run': 'Chạy kiểm tra sâu',
  'nq_doctor_evidence':
      'Các kiểm tra này không thể xác nhận có rò rỉ DNS hay không.',
};

const Map<String, String> kWindowsRecoveryVi = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Cập nhật đồng thời các thành phần Usque trong Cài đặt. Chưa có kết nối VPN mới nào được khởi động.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'Chưa dọn dẹp xong kết nối trước. Thoát hoàn toàn Usque, mở lại rồi thử lại. Nếu vẫn gặp lỗi, hãy mở Chẩn đoán.',
  'WINDOWS_RECOVERY_FAILED':
      'Không thể khôi phục đầy đủ trạng thái mạng VPN trước đó. Chưa khởi động kết nối VPN mới. Hãy thử kết nối lại hoặc xem chẩn đoán cục bộ.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows không khôi phục được trạng thái mạng VPN trước đó sau ba lần thử tự động. Hãy thử lại khi sẵn sàng, hoặc xem chẩn đoán cục bộ.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Đã dừng sửa chữa tự động vì chưa xác nhận được việc khôi phục an toàn. Cập nhật Usque trong Cài đặt. Nếu vẫn gặp lỗi, hãy xuất nhật ký trong Chẩn đoán.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Việc khôi phục mạng Windows lâu hơn dự kiến. Chưa khởi động kết nối VPN mới. Hãy đợi khôi phục xong rồi mới thử lại.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Trạng thái mạng đã đổi hoặc phiên khác vẫn đang dùng. Đã dừng khôi phục tự động để bảo vệ kết nối đang hoạt động.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Bản cài đặt này không thể tự khôi phục kết nối VPN trước. Cập nhật Usque trong Cài đặt rồi thử lại.',
};

const String kWindowsAdapterCleanupVi =
    'Không thể gỡ bộ điều hợp mạng ảo của kết nối trước hoặc xác nhận rằng đã gỡ. Chưa có kết nối VPN mới nào được khởi động.';

const Map<String, String> kL4Vi = <String, String>{
  'l4_quic_not_ready': 'Đang chuẩn bị kết nối L4',
  'l4_unsupported_packets': 'Đã từ chối gói không hỗ trợ hoặc sai định dạng',
  'l4_budget_rejections': 'Số lần từ chối cấp tài nguyên',
  'l4_not_applicable': 'Không áp dụng (L4)',
  'l4_mode': 'L4 (thử nghiệm)',
  'l4_transport_hint':
      'Chỉ hỗ trợ TCP. Ứng dụng cần UDP có thể không hoạt động. Chế độ tự động không chọn L4.',
  'l4_explanation':
      'L4 truyền lưu lượng TCP qua HTTP/3 và dùng được với VPN, proxy SOCKS5 và HTTP. Truy vấn DNS của VPN được chuyển sang TCP. Không hỗ trợ lưu lượng UDP khác, Ping từ xa, các mảnh và phần mở rộng IP; một số ứng dụng có thể không hoạt động. Bạn cần tự chọn L4, ứng dụng không tự bật chế độ này.',
  'l4_unsupported':
      'Phiên bản này không hỗ trợ L4. Cập nhật Usque trong Cài đặt.',
  'l4_sni_identity':
      'Tên máy chủ được tài khoản tự động đặt. Tên máy chủ đã lưu cho các chế độ kết nối khác vẫn được giữ lại.',
  'l4_edge_requires_l4':
      'DNS phân giải ở biên yêu cầu L4. Hãy chọn chế độ DNS proxy khác trước khi chuyển sang Auto, H3 hoặc H2.',
  'proxy_dns_edge_resolved': 'Biên Cloudflare (chỉ L4; không tra cứu cục bộ)',
  'l4_verified': 'Đã thiết lập kết nối ứng dụng qua L4',
  'l4_unverified': 'Đã kết nối máy chủ; chưa xác nhận kết nối ứng dụng',
  'l4_status_unknown': 'Không có trạng thái kết nối ứng dụng',
  'l4_sessions': 'Phiên / đang xả',
  'l4_flows': 'Luồng đang chạy / đang chờ',
  'l4_connect': 'CONNECT thành công / thất bại / hết hạn',
  'l4_buffers': 'Ngân sách bộ đệm ứng dụng đã dùng (byte)',
  'l4_backpressure': 'Áp lực ngược gửi / nhận',
  'l4_tun_flows': 'TUN TCP / nửa mở',
  'l4_udp': 'Gói UDP bị từ chối',
  'l4_dns': 'Chuyển DNS thành công / thất bại / hết hạn',
  'l4_migration': 'Luồng giữ nhờ chuyển đường / kết thúc do dựng lại',
  'l4_na':
      'Điều khiển địa chỉ CONNECT-IP, hàng đợi DATAGRAM, MTU tải trọng trong và thời hạn UDP: không áp dụng ở L4.',
};

const Map<String, String> kNetworkSettingsVi = <String, String>{
  'settings_applying': 'Đã lưu, đang áp dụng',
  'settings_applied': 'Đã lưu và áp dụng',
  'settings_deferred': 'Đã lưu, có hiệu lực ở lần kết nối thủ công tiếp theo',
  'settings_failed': 'Đã lưu, áp dụng thất bại',
  'settings_unknown': 'Kết quả chưa được xác nhận',
  'settings_saved': 'Đã lưu',
  'settings_unsupported':
      'Thoát hoàn toàn Usque rồi mở lại, sau đó thử lưu lần nữa. Nếu vẫn gặp lỗi, hãy cập nhật Usque trong Cài đặt.',
  'settings_save_failed':
      'Không lưu được cài đặt. Các chỉnh sửa của bạn vẫn được giữ.',
  'settings_reconnect': 'Kết nối lại',
};
