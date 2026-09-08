/// Supplemental feature strings for Vietnamese.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowVi = <String, String>{
  'cc_label': 'Kiểm soát tắc nghẽn HTTP/3',
  'cc_help': 'Có hiệu lực ở lần kết nối thủ công tiếp theo.',
  'cc_upgrade': 'Cần cập nhật Engine.',
  'cc_h2': 'HTTP/2 dùng TCP của hệ thống.',
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
  'home_traffic_idle': 'Bắt đầu sau khi kết nối',
  'home_traffic_waiting': 'Đang chờ mẫu',
  'home_traffic_unavailable': 'Không có lịch sử',
  'home_traffic_stale': 'Mẫu bị trễ',
  'home_outputs_next': 'Đầu ra sẽ bật sau khi kết nối',
  'home_outputs_retry': 'Đầu ra đã cấu hình cho lần thử tiếp theo',
  'connection_protection_group': 'Kết nối & bảo vệ',
  'proxy_routing_group': 'Proxy & định tuyến',
  'application_group': 'Ứng dụng',
  'proxy_settings_link': 'Địa chỉ trình lắng nghe, cổng, xác thực và DNS.',
  'proxy_auth_separate':
      'Thông tin xác thực được lưu riêng bằng Lưu thông tin xác thực.',
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
      'Đang chọn DNS hệ thống vật lý; kiểm tra DNS mã hóa không áp dụng.',
  'nq_finding_unsupported':
      'Engine này không có DNS mã hóa; không cho phép dự phòng DNS không mã hóa (plaintext).',
  'nq_finding_dns_custom_valid':
      'Cấu hình DNS mã hóa tùy chỉnh hợp lệ. Đã tắt dự phòng DNS không mã hóa (plaintext).',
  'nq_finding_stale': 'Số liệu đã cũ hoặc mạng vật lý đã đổi.',
  'nq_finding_rtt_high': 'Thời gian khứ hồi đo được đang cao.',
  'nq_finding_healthy': 'Phép đo cục bộ hiện có nằm trong phạm vi kỳ vọng.',
  'nq_finding_loss_high': 'Mất gói trong khoảng đo đang cao.',
  'nq_finding_queue_pressure':
      'Một hàng đợi đang chịu áp lực hoặc đã ghi nhận gói bị loại trong kết nối này.',
  'nq_finding_pmtu_degraded': 'Xác thực MTU đường dẫn đã suy giảm.',
  'nq_finding_migration_reconnect':
      'Không thể chuyển đường trên đường này; đổi mạng sẽ kết nối lại hoàn toàn.',
  'nq_finding_dns_changed': 'Chế độ DNS đã lưu khác với kết nối đang chạy.',
  'nq_finding_dns_runtime':
      'DNS mã hóa đã thành công. Trạng thái cục bộ không phải bằng chứng không rò ra ngoài.',
  'nq_finding_dns_degraded':
      'DNS mã hóa đã suy giảm; truy vấn trực tiếp thất bại không quay về DNS hệ thống.',
  'nq_finding_probe_unsafe':
      'Đã bỏ qua đầu dò: thiếu trạng thái an toàn hoặc danh tính đã lưu. Không bao giờ nhân bản đường hầm đang hoạt động.',
  'nq_finding_probe_success':
      'Đầu dò đã xác thực hoàn tất. Đây không phải kiểm tra rò gói bên ngoài.',
  'nq_finding_probe_cancelled': 'Đã hủy đầu dò và yêu cầu dọn dẹp.',
  'nq_finding_probe_timeout': 'Đầu dò có giới hạn không kịp trước hạn.',
  'nq_finding_probe_failed':
      'Đầu dò đã xác thực thất bại; không thử đường không an toàn.',
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
      'Engine này không cung cấp chất lượng mạng. Các điều khiển kết nối hiện có vẫn hoạt động.',
  'nq_empty':
      'Kết nối để xem phép đo. Giá trị không rõ không hiện thành số không.',
  'nq_stale_help':
      'Nguồn đã ngừng cập nhật. Đây là số liệu trước; chỗ trống vẫn để trống.',
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
  'nq_pmtu_help': 'Khám phá đường không tăng MTU TUN của thiết bị.',
  'nq_migration': 'Chuyển mạng',
  'nq_migration_help':
      'Một kết nối, một đường dữ liệu. Chỉ cùng họ IP; không phải đa đường.',
  'nq_attempts': 'Lần thử',
  'nq_successes': 'Thành công',
  'nq_failures': 'Thất bại',
  'nq_last_duration': 'Thời lượng gần nhất',
  'nq_direct_dns': 'DNS trực tiếp',
  'nq_system_dns': 'DNS hệ thống vật lý',
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
      'Họ IP hiện tại không có; sẽ kết nối lại hoàn toàn.',
  'nq_reason_socket_protect_failed':
      'Không chuẩn bị được socket ứng viên đã bảo vệ.',
  'nq_reason_generation_changed_during_setup':
      'Mạng lại thay đổi trong lúc thiết lập.',
  'nq_reason_peer_cid_unavailable': 'Phía đối không còn mã kết nối dự phòng.',
  'nq_reason_local_cid_unavailable': 'Mã kết nối cục bộ không có sẵn.',
  'nq_reason_path_probe_rejected': 'Không xác thực được đường ứng viên.',
  'nq_reason_path_validation_timeout':
      'Xác thực đường hết thời gian; có thể kết nối lại.',
  'nq_reason_superseded': 'Thay đổi mạng mới hơn đã thay lần thử này.',
  'nq_reason_promotion_failed': 'Không hoàn tất đổi đường một cách an toàn.',
  'nq_reason_connection_closed': 'Kết nối đã đóng trong lúc chuyển.',
  'nq_reason_unsupported': 'Kết nối này không hỗ trợ chuyển đường.',
  'nq_reason_unknown': 'Không có lý do được hỗ trợ.',
  'nq_dns_custom': 'Bộ phân giải mã hóa tùy chỉnh',
  'nq_dns_server': 'Tên máy chủ TLS',
  'nq_dns_path': 'Đường HTTPS',
  'nq_dns_port': 'Cổng (0 dùng mặc định)',
  'nq_dns_bootstrap': 'Địa chỉ IP bootstrap',
  'nq_dns_bootstrap_help':
      'Nhập 1–8 địa chỉ IP dạng số, mỗi dòng một địa chỉ. Không tra cứu tên máy.',
  'nq_dns_no_fallback':
      'Nếu DNS trực tiếp mã hóa thất bại, truy vấn thất bại. Không bao giờ quay về DNS hệ thống hoặc DNS không mã hóa (plaintext).',
  'nq_dns_system_privacy':
      'DNS hệ thống vật lý có thể lộ tên truy vấn trực tiếp cho nhà cung cấp DNS của mạng vật lý.',
  'nq_dns_scope':
      'Chỉ dùng cho truy vấn trực tiếp do Geo chọn. DNS đường hầm không đổi.',
  'nq_dns_no_capability':
      'Engine này không dùng được DNS trực tiếp mã hóa. Cài đặt đã lưu được giữ. Bạn có thể chủ động chọn DNS hệ thống.',
  'nq_dns_invalid_name':
      'Nhập tên DNS không có khoảng trắng, cú pháp URL hay ký tự đại diện.',
  'nq_dns_invalid_path':
      'Dùng /path tối đa 256 ký tự, không có truy vấn, phân mảnh hay khoảng trắng.',
  'nq_dns_invalid_bootstrap':
      'Dùng 1–8 IP unicast không trùng; không dùng địa chỉ chưa chỉ định, multicast, broadcast hoặc link-local IPv6.',
  'nq_dns_invalid_port': 'Nhập 0–65535.',
  'nq_dns_invalid_mode': 'Chọn chế độ DNS được hỗ trợ.',
  'nq_doctor_deep_title': 'Chạy kiểm tra mạng sâu?',
  'nq_doctor_deep_body':
      'Kiểm tra sâu có thể gửi truy vấn DNS thử tới bộ phân giải đã cấu hình và xác thực một đường QUIC được bảo vệ. Kéo dài tối đa 15 giây, có thể hủy, không bao giờ tạo đường hầm dữ liệu thứ hai và không bao giờ đổi DNS, tuyến, tài khoản hay truyền tải của bạn.',
  'nq_doctor_deep_run': 'Chạy kiểm tra sâu',
  'nq_doctor_evidence':
      'Kiểm tra cục bộ mô tả cấu hình và trạng thái quan sát được. Chúng không phải bằng chứng bên ngoài là không rò DNS.',
};

const Map<String, String> kWindowsRecoveryVi = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'Không thể khôi phục đầy đủ trạng thái mạng VPN trước đó. Chưa khởi động kết nối VPN mới. Hãy thử kết nối lại hoặc xem chẩn đoán cục bộ.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows không khôi phục được trạng thái mạng VPN trước đó sau ba lần thử tự động. Hãy thử lại khi sẵn sàng, hoặc xem chẩn đoán cục bộ.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Đã dừng sửa tự động vì không xác minh an toàn được trạng thái mạng Windows trước đó. Hãy khởi động lại Agent hoặc cập nhật Usque, rồi xem chẩn đoán cục bộ.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Việc khôi phục mạng Windows lâu hơn dự kiến. Chưa khởi động kết nối VPN mới. Hãy đợi khôi phục xong rồi mới thử lại.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Trạng thái mạng đã đổi hoặc phiên khác vẫn đang dùng. Đã dừng khôi phục tự động để bảo vệ kết nối đang hoạt động.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Agent Windows này không hỗ trợ khôi phục tự động an toàn. Hãy cập nhật ứng dụng và Agent cùng lúc, rồi thử lại.',
};

const String kWindowsAdapterCleanupVi =
    'Không thể gỡ bộ điều hợp Wintun trước đó hoặc không xác nhận được việc đã gỡ. Chưa khởi động kết nối VPN mới.';
