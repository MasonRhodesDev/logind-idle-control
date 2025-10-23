#!/bin/bash
# Monitor D-Bus StateChanged signals to see what boolean is sent
gdbus monitor --session --dest com.logind.IdleControl 2>&1 &
GDBUS_PID=$!
sleep 2
echo "=== Toggling (current state: $(logind-idle-control status)) ==="
logind-idle-control toggle
sleep 2
echo "=== Toggling again (current state: $(logind-idle-control status)) ==="
logind-idle-control toggle
sleep 2
kill $GDBUS_PID 2>/dev/null || true
