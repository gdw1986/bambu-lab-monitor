#!/usr/bin/env python3
"""Bambu Lab Printer Monitor — Local Server
Connects to printer via MQTT (TLS), pushes status to browser via SSE.
"""

import json
import os
import re
import threading
import time
import logging
from flask import Flask, Response, render_template, request, jsonify
import ssl
import paho.mqtt.client as mqtt

logging.basicConfig(level=logging.INFO, format='%(asctime)s %(message)s')
log = logging.getLogger(__name__)

app = Flask(__name__)

# ── Config ──────────────────────────────────────────────────────────────────
# 填写你的打印机信息
PRINTER_HOST   = os.environ.get("BAMBU_IP", "192.168.1.87")   # 打印机局域网 IP
PRINTER_PORT   = 8883
PRINTER_SERIAL = os.environ.get("BAMBU_SN", "YOUR_SERIAL")    # 设备序列号（在打印机设置里找）
PRINTER_ACCESS_CODE = os.environ.get("BAMBU_CODE", "YOUR_ACCESS_CODE")  # 访问码（在打印机设置里找，须开通）
# ───────────────────────────────────────────────────────────────────────────

# In-memory latest state, shared across threads
latest = {
    "mode": "unknown",
    "action": "unknown",
    "gcode_state": "UNKNOWN",
    "progress": 0,
    "remaining_time": 0,
    "nozzle_temp": 0,
    "nozzle_target": 0,
    "bed_temp": 0,
    "bed_target": 0,
    "layer_current": 0,
    "layer_total": 0,
    "speed": 100,
    "filament_type": "",
    "ams": {},
    "job_name": "",
    "live_speed": 0,
    "light": "off",
    "online": True,
    "last_update": None,
}

sse_clients = []
sse_lock = threading.Lock()
_mqtt_client = None
_mqtt_connected = False


def broadcast(data: dict):
    payload = "data: " + json.dumps(data) + chr(10) + chr(10)
    dead = []
    with sse_lock:
        for i, env in enumerate(sse_clients):
            try:
                env['queue'].put_nowait(payload)
            except:
                dead.append(i)
        for i in reversed(dead):
            sse_clients.pop(i)


# ── MQTT ────────────────────────────────────────────────────────────────────
def parse_payload(raw: bytes) -> dict:
    try:
        # Bambu sends JSON in the payload
        obj = json.loads(raw.decode('utf-8'))
        # Wrap in list if needed
        if isinstance(obj, list):
            obj = obj[0] if obj else {}
        return obj
    except Exception:
        return {}


def extract_value(obj: dict, *keys, default=None):
    for k in keys:
        if isinstance(obj, dict):
            obj = obj.get(k, {})
        else:
            return default
    return obj if obj != {} else default


def on_connect(client, userdata, flags, rc, properties=None, **kwargs):
    if rc == 0:
        log.info("MQTT connected ✓")
        topic = f"device/{PRINTER_SERIAL}/report"
        client.subscribe(topic, qos=1)
        log.info(f"Subscribed to {topic}")
        # Request full status push from printer
        req_topic = f"device/{PRINTER_SERIAL}/request"
        client.publish(req_topic, json.dumps({"pushing": {"sequence_id": "0", "command": "pushall"}}), qos=1)
        client.publish(req_topic, json.dumps({"info": {"sequence_id": "0", "command": "get_version"}}), qos=1)
        global _mqtt_client, _mqtt_connected
        _mqtt_client = client
        _mqtt_connected = True
        latest['online'] = True
        broadcast(latest.copy())
    else:
        log.error(f"MQTT connect failed rc={rc}")
        latest['online'] = False


def on_disconnect(client, userdata, disconnect_flags, rc, properties=None):
    log.warning(f"MQTT disconnected rc={rc}")
    global _mqtt_connected
    _mqtt_connected = False
    latest['online'] = False
    broadcast({"online": False})


def on_message(client, userdata, msg):
    global latest
    try:
        obj = json.loads(msg.payload.decode('utf-8'))
    except:
        return

    # pushall full response: {"print": {...}} — has all fields including gcode_state, progress, AMS
    # info version response: {"info": {...}}
    # periodic short push: {"print": {"nozzle_temper":..., "command":"push_status", "msg":1}}
    print_data = obj.get("print", {})

    # gcode_state
    gcode_state = str(print_data.get("gcode_state", "UNKNOWN"))
    # remaining_time: seconds
    remaining = int(print_data.get("mc_remaining_time", 0) or 0)
    # progress: mc_percent is 0-100 (integer)
    progress = float(print_data.get("mc_percent", 0) or 0)
    # layers
    layer_c = int(print_data.get("layer_num", 0) or 0)
    layer_t = int(print_data.get("total_layer_num", 0) or 0)

    # job_name / gcode_file
    job_name = print_data.get("subtask_name") or print_data.get("gcode_file") or ""
    if job_name:
        # strip file extension
        job_name = re.sub(r'\.3mf$', '', job_name)

    # nozzle / bed temps
    nozzle_t   = float(print_data.get("nozzle_temper", 0) or 0)
    nozzle_tgt = float(print_data.get("nozzle_target_temper", 0) or 0)
    bed_t      = float(print_data.get("bed_temper", 0) or 0)
    bed_tgt    = float(print_data.get("bed_target_temper", 0) or 0)

    # speed
    spd_lvl = print_data.get("spd_lvl", 2)
    if isinstance(spd_lvl, int):
        spd_lvl = f"{spd_lvl}"

    # fan speed (used as live_speed proxy)
    live_speed = int(print_data.get("fan_gear", 0) or 0)

    # light
    lights = print_data.get("lights_report", [])
    light = "off"
    for l in lights:
        if isinstance(l, dict) and l.get("node") == "chamber_light":
            light = l.get("mode", "off")

    # AMS — full format from pushall
    ams = {}
    ams_data = print_data.get("ams", {})
    ams_list = ams_data.get("ams", [])
    if isinstance(ams_list, list):
        for slot in ams_list[:4]:
            if isinstance(slot, dict):
                slot_id = slot.get("id", "?")
                trays = slot.get("tray", [])
                for tray in trays[:1]:  # one tray per AMS slot
                    if isinstance(tray, dict):
                        color_hex = tray.get("tray_color", "N/A")
                        # parse color
                        if color_hex and len(color_hex) >= 6:
                            r = int(color_hex[0:2], 16) if color_hex[0:2] != "FF" else 255
                            g = int(color_hex[2:4], 16) if color_hex[2:4] != "FF" else 255
                            b = int(color_hex[4:6], 16) if color_hex[4:6] != "FF" else 255
                            color = f"#{r:02X}{g:02X}{b:02X}"
                        else:
                            color = color_hex
                        ams[f"slot{slot_id}"] = {
                            "color": color,
                            "material": tray.get("tray_type", "N/A"),
                            "remaining": int(tray.get("remain", 0) or 0),
                        }

    # AMS filament type
    filament = ""
    tray_now = ams_data.get("tray_now", "")
    tray_list = ams_data.get("ams", [])
    if tray_now and tray_list:
        for slot in tray_list:
            if slot.get("id") == str(tray_now):
                trays = slot.get("tray", [])
                for tray in trays:
                    if tray.get("id") == str(ams_data.get("tray_tar", "")):
                        filament = tray.get("tray_type", "")
                        break

    # Detect if this is a full pushall response (has gcode_state) or a short periodic push
    is_full = bool(print_data.get("gcode_state"))

    if is_full:
        # Full pushall: update everything
        global latest
        latest.update({
            "mode": "printer",
            "action": print_data.get("command", ""),
            "gcode_state": gcode_state,
            "progress": progress,
            "remaining_time": remaining,
            "layer_current": layer_c,
            "layer_total": layer_t,
            "nozzle_temp": round(nozzle_t, 1),
            "bed_temp": round(bed_t, 1),
            "live_speed": live_speed,
            "speed": spd_lvl,
            "filament_type": filament,
            "ams": ams,
            "job_name": job_name,
            "light": light,
            "online": True,
            "last_update": time.strftime("%H:%M:%S"),
        })
    else:
        # Short periodic push: only update fields that are present, preserve full state
        updates = {"online": True, "last_update": time.strftime("%H:%M:%S")}
        # Only update temps if the fields are present in this push
        if "nozzle_temper" in print_data:
            updates["nozzle_temp"] = round(nozzle_t, 1)
        if "bed_temper" in print_data:
            updates["bed_temp"] = round(bed_t, 1)
        if "nozzle_target_temper" in print_data:
            updates["nozzle_target"] = round(nozzle_tgt, 1)
        if "bed_target_temper" in print_data:
            updates["bed_target"] = round(bed_tgt, 1)
        if remaining:
            updates["remaining_time"] = remaining
        elif "mc_remaining_time" not in print_data:
            pass  # preserve old value
        latest.update(updates)
    broadcast(latest.copy())




def start_mqtt():
    client = mqtt.Client(
        callback_api_version=mqtt.CallbackAPIVersion.VERSION2,
        protocol=mqtt.MQTTv311,
    )
    client.username_pw_set("bblp", PRINTER_ACCESS_CODE or None)
    # Skip SSL cert verify — printer uses self-signed cert (paho-mqtt 2.x)
    ssl_ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_CLIENT)
    ssl_ctx.check_hostname = False
    ssl_ctx.verify_mode = ssl.CERT_NONE
    client.tls_set_context(ssl_ctx)
    client.on_connect    = on_connect
    client.on_disconnect = on_disconnect
    client.on_message    = on_message

    log.info(f"Connecting to {PRINTER_HOST}:{PRINTER_PORT} ...")
    client.connect(PRINTER_HOST, PRINTER_PORT, keepalive=30)
    client.loop_start()


# ── SSE endpoint ────────────────────────────────────────────────────────────
@app.route('/events')
@app.route('/events')
def sse():
    queue = __import__('queue').Queue()
    with sse_lock:
        sse_clients.append({'queue': queue, 'ip': request.remote_addr})
    # Send current state immediately so browser doesn't flash "离线"
    payload = "data: " + json.dumps(latest.copy()) + chr(10) + chr(10)
    try:
        queue.put_nowait(payload)
    except:
        pass

    def generate():
        while True:
            try:
                data = queue.get(timeout=60)
                yield data
            except:
                break
        with sse_lock:
            sse_clients[:] = [c for c in sse_clients if c['queue'] is not queue]

    return Response(generate(), mimetype='text/event-stream')


@app.route('/api/status')
def api_status():
    return jsonify(latest.copy())

@app.route('/api/config', methods=['GET', 'POST'])
def api_config():
    global PRINTER_HOST, PRINTER_SERIAL, PRINTER_ACCESS_CODE, _mqtt_client, _mqtt_connected
    if request.method == 'GET':
        return jsonify({"host": PRINTER_HOST, "serial": PRINTER_SERIAL,
                         "has_access_code": bool(PRINTER_ACCESS_CODE)})
    d = request.json or {}
    PRINTER_HOST        = d.get('host', PRINTER_HOST)
    PRINTER_SERIAL      = d.get('serial', PRINTER_SERIAL)
    PRINTER_ACCESS_CODE = d.get('access_code', '')  # optional, defaults to empty
    log.info(f"Config updated: host={PRINTER_HOST} serial={PRINTER_SERIAL}")
    if _mqtt_client:
        try: _mqtt_client.loop_stop(); _mqtt_client.disconnect()
        except: pass
    threading.Timer(1, start_mqtt).start()
    return jsonify({"ok": True, "host": PRINTER_HOST, "serial": PRINTER_SERIAL})


# ── Pages ────────────────────────────────────────────────────────────────────
@app.route('/')
def index():
    return render_template('index.html')


if __name__ == '__main__':
    print(f"Dashboard: http://localhost:5001")
    try:
        start_mqtt()
    except Exception as e:
        print(f"[demo mode] MQTT unavailable: {e}")
        print("         Starting in demo mode. Edit server.py to configure printer.")
    app.run(host='0.0.0.0', port=5001, threaded=True, debug=False)

def request_full_status():
    """Request full status push from printer"""
    if _mqtt_client and _mqtt_connected:
        topic = f"device/{PRINTER_SERIAL}/request"
        # Try multiple command formats
        for cmd in [
            '{"pushing": "all"}',
            '{"info": {"push_all": true}}',
            '{"cmd": "push_all"}',
        ]:
            result = _mqtt_client.publish(topic, cmd, qos=1)
            log.info(f"Published to {topic}: {cmd}, rc={result.rc}")

