#!/bin/bash
./target/release/logind-idle-control monitor > /tmp/monitor-output.txt 2>&1 &
MONITOR_PID=$!
sleep 1
echo "Initial state file: $(cat /run/user/1000/logind-idle-control-session-2.state)"
echo "=== Toggling ==="
./target/release/logind-idle-control toggle
sleep 1
echo "After toggle 1: $(cat /run/user/1000/logind-idle-control-session-2.state)"
echo "=== Toggling again ==="
./target/release/logind-idle-control toggle  
sleep 1
echo "After toggle 2: $(cat /run/user/1000/logind-idle-control-session-2.state)"
kill $MONITOR_PID
wait $MONITOR_PID 2>/dev/null || true
echo "=== Monitor output ==="
cat /tmp/monitor-output.txt
rm /tmp/monitor-output.txt
