/// Supplemental feature strings for Thai.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowTh = <String, String>{
  'cc_label': 'การควบคุมความแออัด HTTP/3',
  'cc_help': 'มีผลเมื่อคุณเชื่อมต่อด้วยตนเองครั้งถัดไป',
  'cc_upgrade': 'ต้องอัปเดต Engine',
  'cc_h2': 'HTTP/2 ใช้ TCP ของระบบ',
  'cc_saved': 'บันทึกแล้ว',
  'cc_pending': 'รอการเชื่อมต่อด้วยตนเองครั้งถัดไป',
  'save_changes': 'ใช้การเปลี่ยนแปลง',
  'saving_changes': 'กำลังใช้การเปลี่ยนแปลง…',
  'unsaved_changes': 'การเปลี่ยนแปลงที่ยังไม่ได้ใช้',
  'changes_applied': 'ใช้การเปลี่ยนแปลงแล้ว',
  'changes_apply_hint': 'การแก้ไขจะมีผลหลังจากที่คุณใช้การเปลี่ยนแปลงเท่านั้น',
  'changes_failed': 'ใช้การเปลี่ยนแปลงไม่ได้ ตรวจค่าที่บันทึกไว้แล้วลองใหม่',
  'form_errors': 'ตรวจช่องที่ไฮไลต์ก่อนใช้การเปลี่ยนแปลง',
  'discard_changes_title': 'ละทิ้งการเปลี่ยนแปลงที่ยังไม่ได้ใช้หรือไม่',
  'discard_changes_body':
      'การแก้ไขของคุณยังไม่ได้ถูกใช้ แก้ไขต่อเพื่อบันทึก หรือละทิ้งแล้วออก',
  'keep_editing': 'แก้ไขต่อ',
  'discard_changes': 'ละทิ้งการเปลี่ยนแปลง',
  'invalid_port': 'กรอกพอร์ตระหว่าง 1 ถึง 65535',
  'listener_exposure': 'ที่อยู่ตัวรับฟังอนุญาตให้เข้าถึงจากเครือข่ายภายใน',
  'invalid_ipv4': 'กรอกที่อยู่ IPv4 ที่ถูกต้อง เช่น 127.0.0.1',
  'invalid_ipv6': 'กรอกที่อยู่ IPv6 ที่ถูกต้อง เช่น ::1',
  'output_running': 'กำลังทำงาน',
  'output_waiting': 'เปิดใช้ · ยังไม่ทำงาน',
  'output_disabled': 'ปิดอยู่',
  'output_starting': 'กำลังเริ่ม',
  'output_stopping': 'กำลังหยุด',
  'output_reconnecting': 'กำลังเชื่อมต่อใหม่',
  'output_degraded': 'จำกัด',
  'output_error': 'ข้อผิดพลาด',
  'output_unknown': 'ไม่มีสถานะ',
  'shared_network_scope': 'การตั้งค่าเครือข่ายใช้ร่วมกันทุกบัญชี',
  'connection_details': 'รายละเอียดการเชื่อมต่อ',
  'home_overview': 'ภาพรวมการเชื่อมต่อ',
  'home_exit_region': 'ภูมิภาคทางออก',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'ทราฟฟิก',
  'home_traffic_window': '60 วินาทีล่าสุด',
  'home_traffic_idle': 'เริ่มหลังเชื่อมต่อ',
  'home_traffic_waiting': 'กำลังรอตัวอย่าง',
  'home_traffic_unavailable': 'ไม่มีประวัติ',
  'home_traffic_stale': 'ตัวอย่างล่าช้า',
  'home_outputs_next': 'จะเปิดเอาต์พุตหลังเชื่อมต่อ',
  'home_outputs_retry': 'ตั้งค่าเอาต์พุตสำหรับครั้งถัดไป',
  'connection_protection_group': 'การเชื่อมต่อและการป้องกัน',
  'proxy_routing_group': 'พร็อกซีและการกำหนดเส้นทาง',
  'application_group': 'แอป',
  'proxy_settings_link': 'ที่อยู่ตัวรับฟัง พอร์ต การยืนยันตัวตน และ DNS',
  'proxy_auth_separate': 'ข้อมูลรับรองถูกบันทึกแยกด้วย บันทึกข้อมูลรับรอง',
  'reset_draft_hint':
      'ค่าเริ่มต้นจะถูกโหลดลงในแบบฟอร์มนี้ ใช้การเปลี่ยนแปลงจึงจะมีผล',
};

const Map<String, String> kNetworkQualityTh = <String, String>{
  'nq_range': 'ช่วง',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'เวลาไป-กลับ',
  'diag_check_quality_packet_loss': 'การสูญเสียแพ็กเก็ต',
  'diag_check_quality_queue_pressure': 'ความดันของคิว',
  'diag_check_quality_pmtu': 'MTU ของเส้นทาง',
  'diag_check_transport_migration_capability': 'การย้ายในตระกูลเดียวกัน',
  'diag_check_dns_direct_encrypted_configuration': 'การกำหนดค่า DNS ตรง',
  'diag_check_dns_direct_encrypted_runtime_state': 'สถานะการทำงาน DNS ตรง',
  'diag_check_dns_direct_encrypted_reachability': 'การเข้าถึง DNS แบบเข้ารหัส',
  'diag_check_transport_h3_path_validation_probe': 'การจับมือ QUIC แบบแยก',
  'nq_finding_unavailable': 'การวัดนี้ไม่พร้อมใช้งานในสถานะปัจจุบัน',
  'nq_finding_invalid_configuration': 'การกำหนดค่า DNS ที่กำหนดเองไม่ถูกต้อง',
  'nq_finding_dns_system':
      'เลือก DNS ของระบบกายภาพแล้ว การตรวจ DNS แบบเข้ารหัสจึงไม่ใช้',
  'nq_finding_unsupported':
      'Engine นี้ไม่มี DNS แบบเข้ารหัส และไม่อนุญาตให้ถอยกลับเป็นข้อความธรรมดา',
  'nq_finding_dns_custom_valid':
      'การกำหนดค่า DNS แบบเข้ารหัสที่กำหนดเองถูกต้อง การถอยกลับเป็นข้อความธรรมดาถูกปิด',
  'nq_finding_stale': 'ค่าที่อ่านล้าสมัย หรือเครือข่ายกายภาพเปลี่ยนแล้ว',
  'nq_finding_rtt_high': 'เวลาไป-กลับที่วัดได้สูงกว่าปกติ',
  'nq_finding_healthy': 'ค่าที่วัดในเครื่องที่พร้อมใช้อยู่ในช่วงที่คาดไว้',
  'nq_finding_loss_high': 'การสูญเสียแพ็กเก็ตในช่วงนี้สูงกว่าปกติ',
  'nq_finding_queue_pressure':
      'มีคิวที่รับภาระสูง หรือมีการทิ้งระหว่างการเชื่อมต่อนี้',
  'nq_finding_pmtu_degraded': 'การตรวจสอบ MTU ของเส้นทางลดระดับแล้ว',
  'nq_finding_migration_reconnect':
      'เส้นทางนี้ย้ายไม่ได้ การเปลี่ยนเครือข่ายจะเชื่อมต่อใหม่ทั้งชุด',
  'nq_finding_dns_changed':
      'โหมด DNS ที่บันทึกไว้ต่างจากการเชื่อมต่อที่กำลังทำงาน',
  'nq_finding_dns_runtime':
      'DNS แบบเข้ารหัสสำเร็จแล้ว สถานะในเครื่องไม่ใช่หลักฐานว่าไม่รั่วออกภายนอก',
  'nq_finding_dns_degraded':
      'DNS แบบเข้ารหัสลดระดับแล้ว คำขอตรงที่ล้มเหลวจะไม่ถอยกลับไป DNS ของระบบ',
  'nq_finding_probe_unsafe':
      'ข้ามโพรบ: ไม่มีสถานะที่ปลอดภัยหรือตัวตนที่บันทึกไว้ จะไม่ทำสำเนาอุโมงค์ที่กำลังทำงาน',
  'nq_finding_probe_success':
      'โพรบที่ยืนยันตัวตนเสร็จแล้ว นี่ไม่ใช่การทดสอบการรั่วของแพ็กเก็ตภายนอก',
  'nq_finding_probe_cancelled': 'ยกเลิกโพรบแล้ว และขอให้ทำความสะอาด',
  'nq_finding_probe_timeout': 'โพรบที่มีเวลาจำกัดไม่ทันกำหนด',
  'nq_finding_probe_failed':
      'โพรบที่ยืนยันตัวตนล้มเหลว ไม่ได้ลองเส้นทางที่ไม่ปลอดภัย',
  'diag_fix_nq_profile':
      'ตรวจช่อง DNS ที่กำหนดเองและชื่อใบรับรอง อย่าปิดการตรวจสอบ TLS',
  'diag_fix_nq_retry': 'รอให้เครือข่ายเสถียร แล้วลองใหม่',
  'diag_fix_nq_network':
      'ตรวจการเชื่อมต่อในเครื่อง และเปรียบเทียบตัวอย่างใหม่ก่อนเปลี่ยนการตั้งค่า',
  'diag_fix_nq_reconnect': 'เชื่อมต่อใหม่เพื่อใช้การกำหนดค่าที่บันทึกไว้',
  'nav_network_quality': 'คุณภาพ',
  'network_quality': 'คุณภาพเครือข่าย',
  'nq_subtitle': 'อ่านสถานะการเชื่อมต่อ ไม่ใช่แค่ความเร็ว',
  'nq_local_only': 'วัดเฉพาะในเครื่อง ไม่มีการอัปโหลด',
  'nq_doctor': 'เรียกใช้ตัวตรวจเครือข่าย',
  'nq_doctor_help':
      'การตรวจมาตรฐานอ่านสถานะในเครื่องเท่านั้น ไม่เปิดการเชื่อมต่อภายนอก และไม่เปลี่ยนการตั้งค่าของคุณ',
  'nq_live': 'สด',
  'nq_stale': 'ค่าที่อ่านล้าสมัย',
  'nq_updated': 'ตัวอย่างล่าสุด',
  'nq_seconds': '{count} วินาทีที่แล้ว',
  'nq_good': 'ดี',
  'nq_fair': 'พอใช้',
  'nq_poor': 'แย่',
  'nq_limited': 'ข้อมูลจำกัด',
  'nq_disconnected': 'ไม่ได้เชื่อมต่อ',
  'nq_connecting': 'กำลังเชื่อมต่อ',
  'nq_connected': 'เชื่อมต่อแล้ว',
  'nq_unavailable': 'ไม่พร้อมใช้งาน',
  'nq_not_ready': 'ยังไม่พร้อม',
  'nq_unsupported': 'ไม่รองรับ',
  'nq_capability_missing':
      'Engine นี้ไม่มีข้อมูลคุณภาพเครือข่าย ตัวควบคุมการเชื่อมต่อที่มีอยู่ยังใช้ได้',
  'nq_empty': 'เชื่อมต่อเพื่อดูค่าที่วัด ไม่แสดงค่าที่ไม่ทราบเป็นศูนย์',
  'nq_stale_help':
      'แหล่งข้อมูลหยุดอัปเดต นี่เป็นค่าก่อนหน้า ช่องว่างยังคงเป็นช่องว่าง',
  'nq_rtt': 'เวลาไป-กลับ',
  'nq_latest': 'ล่าสุด',
  'nq_smoothed': 'ค่าเรียบ',
  'nq_minimum': 'ค่าต่ำสุด',
  'nq_h2_ping': 'PING โปรโตคอล HTTP/2',
  'nq_h3_rtt': 'การวัดเส้นทาง QUIC',
  'nq_throughput': 'อัตราผ่าน',
  'nq_download': 'ดาวน์โหลด',
  'nq_upload': 'อัปโหลด',
  'nq_one_second': '1 วินาที',
  'nq_five_seconds': 'ค่าเฉลี่ย 5 วินาที',
  'nq_loss': 'การสูญเสียแพ็กเก็ต',
  'nq_loss_h2': 'HTTP/2 ไม่เปิดเผยการสูญเสียแพ็กเก็ตที่เปรียบเทียบได้',
  'nq_loss_interval': 'วัดในช่วงล่าสุด ไม่ใช่การสูญเสียตลอดอายุ',
  'nq_congestion': 'ความแออัด',
  'nq_cwnd': 'หน้าต่างความแออัด',
  'nq_in_flight': 'Bytes ที่กำลังส่ง',
  'nq_send_rate': 'อัตราการส่งมอบ',
  'nq_h2_window': 'หน้าต่างรับ HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'การเชื่อมต่อ',
  'nq_stalls': 'การหยุดเพราะความจุ',
  'nq_pmtu': 'MTU ของเส้นทาง',
  'nq_outer_pmtu': 'ขีดจำกัดข้อมูล UDP ชั้นนอก',
  'nq_inner_payload': 'ขีดจำกัดข้อมูล CONNECT-IP',
  'nq_pmtu_help': 'การค้นหาเส้นทางจะไม่เพิ่ม MTU ของ TUN บนอุปกรณ์',
  'nq_migration': 'การย้ายเครือข่าย',
  'nq_migration_help':
      'หนึ่งการเชื่อมต่อ หนึ่งเส้นทางข้อมูล เฉพาะตระกูล IP เดียวกัน ไม่ใช่หลายเส้นทาง',
  'nq_attempts': 'ครั้งที่ลอง',
  'nq_successes': 'สำเร็จ',
  'nq_failures': 'ล้มเหลว',
  'nq_last_duration': 'ระยะเวลาล่าสุด',
  'nq_direct_dns': 'DNS ตรง',
  'nq_system_dns': 'DNS ของระบบกายภาพ',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'พร้อม',
  'nq_degraded': 'ลดระดับ',
  'nq_timeouts': 'หมดเวลา',
  'nq_last_rtt': 'RTT ล่าสุด',
  'nq_dns_redacted': 'ชื่อตัวแปลงชื่อและที่อยู่บูตสแตรปแสดงเฉพาะในการตั้งค่า',
  'nq_queues': 'ความดันของคิว',
  'nq_queue_details': 'คิวระดับล่าง',
  'nq_queue_empty': 'ยังไม่มีการวัดคิว',
  'nq_current_capacity': 'ปัจจุบัน / ความจุ',
  'nq_high_water': 'ระดับสูงสุด',
  'nq_drops': 'จำนวนที่ทิ้ง',
  'nq_oldest': 'รายการเก่าสุด',
  'nq_tunToTransport': 'อุปกรณ์ → ทรานสปอร์ต',
  'nq_proxyToTransport': 'พร็อกซี → ทรานสปอร์ต',
  'nq_transportOutgoing': 'ทรานสปอร์ตขาออก',
  'nq_h3DatagramSend': 'ดาตาแกรม QUIC',
  'nq_h3WireSend': 'เอาต์พุต UDP',
  'nq_transportToTun': 'ทรานสปอร์ต → อุปกรณ์',
  'nq_transportToProxy': 'ทรานสปอร์ต → พร็อกซี',
  'nq_directDns': 'คำขอ DNS ตรง',
  'nq_unknown_queue': 'คิวอื่น',
  'nq_trends': '60 วินาทีล่าสุด',
  'nq_samples': 'ตัวอย่าง',
  'nq_pause': 'หยุดกราฟชั่วคราว',
  'nq_resume': 'เล่นกราฟต่อ',
  'nq_paused': 'กราฟหยุดชั่วคราว',
  'nq_gaps': 'ตัวอย่างที่ขาดแสดงเป็นช่องว่าง',
  'nq_phase_idle': 'ว่าง',
  'nq_phase_preparing_socket': 'กำลังเตรียมเส้นทาง',
  'nq_phase_probing': 'กำลังโพรบ',
  'nq_phase_validated': 'ตรวจสอบแล้ว',
  'nq_phase_promoting': 'กำลังสลับเส้นทาง',
  'nq_phase_stable': 'คงที่',
  'nq_phase_aborted': 'ยุติแล้ว',
  'nq_phase_revalidating': 'กำลังตรวจสอบซ้ำ',
  'nq_phase_degraded': 'ลดระดับ',
  'nq_phase_unknown': 'ยังไม่พร้อม',
  'nq_phase_unsupported': 'ไม่รองรับ',
  'nq_reason_family_unavailable':
      'ตระกูล IP ปัจจุบันไม่พร้อมใช้งาน จะเชื่อมต่อใหม่ทั้งชุด',
  'nq_reason_socket_protect_failed':
      'เตรียมซ็อกเก็ตตัวเลือกที่ป้องกันไว้ไม่ได้',
  'nq_reason_generation_changed_during_setup':
      'เครือข่ายเปลี่ยนอีกครั้งระหว่างการเตรียม',
  'nq_reason_peer_cid_unavailable': 'ฝั่งตรงข้ามไม่มีตัวระบุการเชื่อมต่อสำรอง',
  'nq_reason_local_cid_unavailable':
      'ตัวระบุการเชื่อมต่อในเครื่องไม่พร้อมใช้งาน',
  'nq_reason_path_probe_rejected': 'ตรวจสอบเส้นทางตัวเลือกไม่ผ่าน',
  'nq_reason_path_validation_timeout':
      'การตรวจสอบเส้นทางหมดเวลา สามารถเชื่อมต่อใหม่ได้',
  'nq_reason_superseded': 'การเปลี่ยนเครือข่ายที่ใหม่กว่าแทนที่ความพยายามนี้',
  'nq_reason_promotion_failed': 'สลับเส้นทางอย่างปลอดภัยไม่สำเร็จ',
  'nq_reason_connection_closed': 'การเชื่อมต่อปิดระหว่างการย้าย',
  'nq_reason_unsupported': 'การเชื่อมต่อนี้ย้ายไม่ได้',
  'nq_reason_unknown': 'ไม่มีสาเหตุที่รองรับ',
  'nq_dns_custom': 'ตัวแปลงชื่อแบบเข้ารหัสที่กำหนดเอง',
  'nq_dns_server': 'ชื่อเซิร์ฟเวอร์ TLS',
  'nq_dns_path': 'เส้นทาง HTTPS',
  'nq_dns_port': 'พอร์ต (0 ใช้ค่าเริ่มต้น)',
  'nq_dns_bootstrap': 'ที่อยู่ IP บูตสแตรป',
  'nq_dns_bootstrap_help':
      'กรอกที่อยู่ IP ตัวเลข 1–8 รายการ บรรทัดละหนึ่ง ไม่ใช้การค้นหาชื่อโฮสต์',
  'nq_dns_no_fallback':
      'หาก DNS ตรงแบบเข้ารหัสล้มเหลว คำขอจะล้มเหลว จะไม่ถอยกลับไป DNS ของระบบหรือข้อความธรรมดา',
  'nq_dns_system_privacy':
      'DNS ของระบบกายภาพอาจเปิดเผยชื่อที่ขอตรงต่อผู้ให้บริการ DNS ของเครือข่ายกายภาพ',
  'nq_dns_scope':
      'ใช้กับคำขอตรงที่เลือกตาม Geo เท่านั้น DNS ของอุโมงค์ไม่เปลี่ยน',
  'nq_dns_no_capability':
      'Engine นี้ใช้ DNS ตรงแบบเข้ารหัสไม่ได้ การตั้งค่าที่บันทึกไว้ยังคงอยู่ คุณเลือก DNS ของระบบได้อย่างชัดเจน',
  'nq_dns_invalid_name':
      'กรอกชื่อ DNS โดยไม่มีช่องว่าง ไวยากรณ์ URL หรืออักขระตัวแทน',
  'nq_dns_invalid_path':
      'ใช้ /path ยาวไม่เกิน 256 ตัวอักษร โดยไม่มีคิวรี ชิ้นส่วน หรือช่องว่าง',
  'nq_dns_invalid_bootstrap':
      'ใช้ IP ยูนิแคสต์ที่ไม่ซ้ำ 1–8 รายการ ห้ามที่อยู่ที่ยังไม่ระบุ มัลติแคสต์ บรอดแคสต์ หรือลิงก์โลคัล IPv6',
  'nq_dns_invalid_port': 'กรอก 0–65535',
  'nq_dns_invalid_mode': 'เลือกโหมด DNS ที่รองรับ',
  'nq_doctor_deep_title': 'เรียกใช้การตรวจเครือข่ายเชิงลึกหรือไม่',
  'nq_doctor_deep_body':
      'การตรวจเชิงลึกอาจส่งคำขอ DNS ทดสอบไปยังตัวแปลงชื่อที่ตั้งไว้ และตรวจสอบเส้นทาง QUIC ที่ป้องกัน ใช้เวลาไม่เกิน 15 วินาที ยกเลิกได้ จะไม่สร้างอุโมงค์ข้อมูลเส้นที่สอง และจะไม่เปลี่ยน DNS, เส้นทาง, บัญชี หรือทรานสปอร์ตของคุณ',
  'nq_doctor_deep_run': 'เรียกใช้การตรวจเชิงลึก',
  'nq_doctor_evidence':
      'การตรวจในเครื่องอธิบายการกำหนดค่าและสถานะที่สังเกตได้ ไม่ใช่หลักฐานภายนอกว่าไม่มี DNS รั่ว',
};

const Map<String, String> kWindowsRecoveryTh = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'ไม่สามารถคืนค่าสถานะเครือข่าย VPN ก่อนหน้าได้ครบ ยังไม่ได้เริ่มการเชื่อมต่อ VPN ใหม่ ลองเชื่อมต่ออีกครั้ง หรือตรวจการวินิจฉัยในเครื่อง',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows คืนค่าสถานะเครือข่าย VPN ก่อนหน้าไม่ได้หลังลองอัตโนมัติสามครั้ง ลองใหม่เมื่อพร้อม หรือตรวจการวินิจฉัยในเครื่อง',
  'WINDOWS_RECOVERY_BLOCKED':
      'หยุดการซ่อมอัตโนมัติ เพราะยืนยันสถานะเครือข่าย Windows ก่อนหน้าอย่างปลอดภัยไม่ได้ เริ่ม Agent ใหม่หรืออัปเดต Usque แล้วตรวจการวินิจฉัยในเครื่อง',
  'WINDOWS_RECOVERY_TIMEOUT':
      'การกู้คืนเครือข่าย Windows ใช้เวลานานกว่าที่คาด ยังไม่ได้เริ่มการเชื่อมต่อ VPN ใหม่ รอให้กู้คืนเสร็จก่อนลองใหม่',
  'WINDOWS_RECOVERY_CONFLICT':
      'สถานะเครือข่ายเปลี่ยนแล้ว หรือเซสชันอื่นยังใช้อยู่ หยุดการกู้คืนอัตโนมัติเพื่อปกป้องการเชื่อมต่อที่ใช้งานอยู่',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Agent ของ Windows นี้ไม่รองรับการกู้คืนอัตโนมัติที่ปลอดภัย อัปเดตแอปและ Agent พร้อมกัน แล้วลองใหม่',
};

const String kWindowsAdapterCleanupTh =
    'อะแดปเตอร์ Wintun ก่อนหน้านี้ถอดออกไม่ได้ หรือยืนยันการถอดออกไม่ได้ ยังไม่ได้เริ่มการเชื่อมต่อ VPN ใหม่';

const Map<String, String> kL4Th = <String, String>{
  'l4_quic_not_ready': 'กำลังรอเซสชัน QUIC ที่พร้อม',
  'l4_unsupported_packets': 'ปฏิเสธแพ็กเก็ตที่ไม่รองรับหรือผิดรูปแบบ',
  'l4_budget_rejections': 'จำนวนครั้งที่ปฏิเสธการรับทรัพยากร',
  'l4_not_applicable': 'ไม่ใช้ได้ (L4)',
  'l4_mode': 'L4 (ทดลอง)',
  'l4_transport_hint':
      'รองรับเฉพาะ TCP; DNS ของ TUN ใช้ TCP โหมด Auto ไม่รวม L4',
  'l4_explanation':
      'เฉพาะ TCP บน HTTP/3 รองรับ VPN/TUN, SOCKS5 และ HTTP; DNS ของ TUN จะถูกแปลงเป็น TCP Auto จะไม่เลือก L4 UDP อื่น, ping ระยะไกล, ชิ้นส่วน IP และส่วนหัวส่วนขยายไม่รองรับ บางแอปอาจใช้ไม่ได้',
  'l4_unsupported':
      'เอนจินนี้ยังไม่ได้ประกาศการรองรับ L4 ครบ ไม่สามารถเปิด L4 ได้',
  'l4_sni_identity':
      'อ่านอย่างเดียว: ได้จากข้อมูลประจำตัวบัญชีที่โหลดแล้ว SNI ของ CONNECT-IP เดิมจะถูกเก็บไว้',
  'l4_edge_requires_l4':
      'DNS ที่แปลงที่ขอบต้องใช้ L4 เลือกโหมด DNS พร็อกซีอื่นก่อนสลับเป็น Auto, H3 หรือ H2',
  'proxy_dns_edge_resolved': 'ขอบ Cloudflare (เฉพาะ L4 ไม่ค้นหาในเครื่อง)',
  'l4_verified': 'ยืนยัน L4 CONNECT แล้ว',
  'l4_unverified': 'QUIC พร้อมแล้ว ยังไม่ได้ยืนยัน L4 CONNECT',
  'l4_status_unknown': 'ไม่ทราบสถานะการยืนยัน L4',
  'l4_sessions': 'เซสชัน / กำลังระบาย',
  'l4_flows': 'สตรีมที่ใช้งาน / ที่รอ',
  'l4_connect': 'CONNECT สำเร็จ / ล้มเหลว / หมดเวลา',
  'l4_buffers': 'งบประมาณบัฟเฟอร์แอปที่ใช้ (ไบต์)',
  'l4_backpressure': 'แรงดันย้อนส่ง / รับ',
  'l4_tun_flows': 'TUN TCP / เปิดครึ่งหนึ่ง',
  'l4_udp': 'แพ็กเก็ต UDP ที่ปฏิเสธ',
  'l4_dns': 'การแปลง DNS สำเร็จ / ล้มเหลว / หมดเวลา',
  'l4_migration': 'สตรีมที่โยกย้ายเก็บไว้ / ที่สร้างใหม่แล้วจบ',
  'l4_na':
      'การควบคุมที่อยู่ CONNECT-IP, คิว DATAGRAM, MTU ของส่วนข้อมูลชั้นใน และหมดเวลา UDP: ไม่ใช้ได้ใน L4',
};

const Map<String, String> kNetworkSettingsTh = <String, String>{
  'settings_applying': 'บันทึกแล้ว กำลังนำไปใช้',
  'settings_applied': 'บันทึกและนำไปใช้แล้ว',
  'settings_deferred': 'บันทึกแล้ว มีผลเมื่อเชื่อมต่อด้วยตนเองครั้งถัดไป',
  'settings_failed': 'บันทึกแล้ว นำไปใช้ไม่สำเร็จ',
  'settings_unknown': 'ยังไม่ยืนยันผลลัพธ์',
  'settings_saved': 'บันทึกแล้ว',
  'settings_unsupported':
      'รีสตาร์ทหรืออัปเดต Engine เพื่อบันทึกการตั้งค่าเครือข่าย',
  'settings_save_failed': 'บันทึกการตั้งค่าไม่ได้ ยังคงการแก้ไขของคุณไว้',
  'settings_reconnect': 'เชื่อมต่อใหม่',
};
