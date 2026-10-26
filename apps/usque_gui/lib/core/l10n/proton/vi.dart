const kProtonVi = <String, String>{
  'fallback_account':
      'Không thể liên lạc với Proton. Hãy chọn tài khoản WARP hiện có rồi thử lại.',
  'fallback_failed':
      'Không thể kết nối qua WARP. Hãy kiểm tra tài khoản WARP đã chọn và mạng rồi thử lại.',
  'fallback_starting': 'Đang kết nối qua WARP…',
  'fallback_active': 'Thao tác tài khoản này dùng WARP, bao gồm cả xác minh.',
  'connection_api':
      'Yêu cầu API Proton thất bại. Hãy thử lại hoặc kiểm tra tài khoản Proton trong Proxy chuỗi.',
  'connection_authentication':
      'Cấp quyền Proton thất bại. Mở Proxy chuỗi và đăng nhập lại vào Proton.',
  'connection_certificate':
      'Proton từ chối chứng chỉ kết nối và khôi phục thất bại. Hãy kết nối lại hoặc đăng nhập lại vào Proton.',
  'connection_restricted':
      'Máy chủ Proton tạm thời bị hạn chế. Hãy thử lại hoặc chọn máy chủ khác.',
  'connection_sessions':
      'Đã đạt giới hạn kết nối đồng thời của Proton. Ngắt một phiên Proton khác rồi thử lại.',
  'connection_key':
      'Proton phát hiện khóa kết nối này trong nhiều phiên. Ngắt phiên còn lại rồi thử lại.',
  'connection_policy':
      'Proton hạn chế kết nối này theo chính sách tài khoản hoặc sử dụng. Kiểm tra tài khoản hoặc chọn máy chủ khác.',
  'connection_server':
      'Máy chủ Proton không thể cấp quyền kết nối. Hãy thử máy chủ khác.',
  'connection_protocol':
      'Không thể xác thực phản hồi kết nối của Proton. Hãy thử lại hoặc chọn máy chủ khác.',
  'connection_transport':
      'Không thể liên lạc với Proton. Kiểm tra mạng hoặc chọn máy chủ khác.',
  'connection_reconnect': 'Đã gia hạn quyền truy cập Proton. Đang kết nối lại…',
  'connection_review':
      'Mở Proxy chuỗi để kiểm tra tài khoản Proton và máy chủ đã chọn.',
  'connection_retry': 'Thử lại kết nối Proton hoặc chọn máy chủ khác.',
  'title': 'Proton VPN',
  'guest': 'Tiếp tục với tư cách khách',
  'login': 'Đăng nhập Proton',
  'username': 'Tên người dùng',
  'password': 'Mật khẩu',
  'totp': 'Mã từ ứng dụng xác thực',
  'submit': 'Tiếp tục',
  'verify': 'Hoàn tất xác minh',
  'verification_hint': 'Hoàn thành câu đố trong cửa sổ xác minh chính thức.',
  'choose_node': 'Chọn máy chủ miễn phí',
  'working': 'Đang chuẩn bị Proton VPN…',
  'waiting_totp': 'Nhập mã từ ứng dụng xác thực để tiếp tục.',
  'waiting_verification': 'Cần xác minh',
  'waiting_node': 'Chọn máy chủ để hoàn tất thiết lập.',
  'completed':
      'Đã lưu tài khoản. Chọn tài khoản và áp dụng thay đổi để dùng Proton VPN.',
  'cancelled': 'Đã hủy thiết lập',
  'failed': 'Thiết lập Proton thất bại. Hãy thử lại khi sẵn sàng.',
  'result_unconfirmed':
      'Kết quả thiết lập Proton chưa được xác nhận. Làm mới trạng thái để tiếp tục hoặc hủy thiết lập hiện tại trước khi bắt đầu lại.',
  'unavailable': 'Proton VPN không khả dụng trong bộ máy này.',
  'bad_credentials':
      'Kiểm tra tên người dùng Proton, mật khẩu hoặc mã xác thực.',
  'rate_limited': 'Proton đã giới hạn yêu cầu này. Hãy đợi trước khi thử lại.',
  'network':
      'Không thể liên lạc với Proton. Kiểm tra mạng hoặc VPN rồi thử lại.',
  'verification_failed':
      'Không thể hoàn tất xác minh. Hãy bắt đầu lại thiết lập.',
  'verification_invalid':
      'Proton từ chối kết quả xác minh. Bắt đầu lại thiết lập để nhận thử thách mới.',
  'accounts': 'Tài khoản Proton',
  'add_account': 'Thêm tài khoản',
  'account_actions': 'Thao tác tài khoản',
  'resolve_on_connect': 'Máy chủ và quyền truy cập được kiểm tra khi kết nối.',
  'account': 'Tài khoản Proton',
  'signed_in': 'Đã đăng nhập',
  'session_requires_login': 'Phiên cần đăng nhập',
  'selected_server': 'Máy chủ',
  'automatic_server': 'Tự động',
  'automatic_hint': 'Chọn máy chủ có độ trễ thấp nhất khi kết nối.',
  'available_servers': 'Máy chủ khả dụng',
  'choose_server': 'Chọn máy chủ',
  'search_servers': 'Tìm theo tên, quốc gia hoặc thành phố',
  'load_servers': 'Tải danh sách máy chủ',
  'change_server': 'Đổi máy chủ',
  'refresh_servers': 'Làm mới danh sách',
  'remove_account': 'Xóa tài khoản',
  'remove_account_confirm':
      'Xóa tài khoản này và lựa chọn máy chủ đã lưu? Đăng xuất cũng thu hồi các phiên mà ứng dụng này đã lưu. Chỉ xóa trên thiết bị sẽ không thay đổi các phiên từ xa. Lựa chọn proxy chuỗi đã lưu sẽ bị xóa.',
  'forget_local': 'Chỉ xóa trên thiết bị',
  'sign_out_remove': 'Đăng xuất và xóa',
  'current_account': 'Tài khoản đang kết nối',
  'saved_account': 'Lựa chọn đã lưu',
  'next_connection': 'Đã lưu lựa chọn máy chủ cho lần kết nối tiếp theo.',
  'disconnect_to_change':
      'Ngắt kết nối tài khoản này trước khi đổi máy chủ hoặc xóa tài khoản.',
  'cleanup_pending':
      'Không thể xác nhận việc thu hồi phiên trước đó. Đăng xuất lại hoặc chọn rõ việc chỉ xóa tài khoản trên thiết bị.',
  'selected_server_unavailable':
      'Máy chủ đã chọn không khả dụng. Chọn máy chủ khác hoặc Tự động.',
  'close': 'Đóng',
  'verification_error': 'Lỗi xác minh',
  'refresh': 'Gia hạn quyền truy cập hoặc đổi máy chủ',
  'logout': 'Đăng xuất và xóa',
  'forget': 'Chỉ xóa trên thiết bị này',
  'forget_confirm':
      'Xóa cấu hình Proton và phiên đã lưu khỏi thiết bị này? Thao tác này không thu hồi phiên Proton từ xa.',
  'removed': 'Đã xóa tài khoản Proton khỏi thiết bị này.',
  'needs_login': 'Đăng nhập lại',
  'expires': 'Quyền truy cập hết hạn',
  'unsupported_second_factor':
      'Tài khoản này yêu cầu phương thức đăng nhập chưa được hỗ trợ ở đây.',
  'reauthentication_required': 'Đăng nhập lại để gia hạn phiên Proton này.',
  'profile_in_use': 'Ngắt kết nối tài khoản này trước khi thay đổi hoặc xóa.',
  'session_busy':
      'Một thao tác Proton khác vẫn đang chạy. Hãy đợi hoặc hủy thao tác đó trước.',
  'api_rejected':
      'Proton từ chối yêu cầu này. Các mã bên dưới xác định phản hồi của máy chủ.',
  'session_not_allowed': 'Proton không cho phép ứng dụng khách này tạo phiên.',
  'credentialless_disabled':
      'Truy cập Proton với tư cách khách hiện không khả dụng.',
  'no_free_nodes':
      'Proton không trả về máy chủ WireGuard khả dụng cho tài khoản này.',
  'invalid_directory': 'Proton trả về danh sách máy chủ không được hỗ trợ.',
  'invalid_client_configuration':
      'Proton không trả về cổng WireGuard có thể sử dụng.',
  'stage': 'Bước thất bại',
  'stage_anonymous_session': 'Tạo phiên ẩn danh',
  'stage_credentialless': 'Tạo tài khoản khách',
  'stage_session_scopes': 'Kiểm tra quyền của khách',
  'stage_protected_user': 'Kiểm tra tài khoản',
  'stage_vpn_permissions': 'Kiểm tra quyền truy cập VPN',
  'stage_session_refresh': 'Gia hạn phiên',
  'stage_srp_info': 'Bắt đầu đăng nhập',
  'stage_srp_proof': 'Xác minh đăng nhập',
  'stage_totp': 'Xác minh mã từ ứng dụng xác thực',
  'stage_directory': 'Tải máy chủ',
  'stage_certificate': 'Yêu cầu cấp quyền VPN',
  'stage_secure_storage': 'Lưu cấu hình',
  'stage_session_revoke': 'Thu hồi phiên',
  'expired':
      'Yêu cầu xác minh hoặc nhập dữ liệu đã hết hạn. Bắt đầu lại thiết lập.',
  'underlay_changed': 'Môi trường mạng đã thay đổi. Bắt đầu lại thiết lập.',
  'remote_unknown': 'Không thể xác nhận việc thu hồi phiên từ xa.',
  'cancelled_remote_unknown':
      'Đã hủy trên thiết bị này. Không thể xác nhận việc thu hồi phiên từ xa.',
  'privacy':
      'Mật khẩu chỉ được dùng để đăng nhập. Các phiên và khóa được bảo vệ trên thiết bị này.',
  'empty':
      'Đăng nhập hoặc tiếp tục với tư cách khách để thêm tài khoản Proton.',
};
