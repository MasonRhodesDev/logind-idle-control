#!/bin/bash
timeout 10 ./target/release/logind-idle-control monitor &
MONITOR_PID=$!
sleep 2
echo "Toggling..."
./target/release/logind-idle-control toggle
sleep 1
echo "Toggling again..."
./target/release/logind-idle-control toggle
sleep 1
kill $MONITOR_PID 2>/dev/null || true
wait $MONITOR_PID 2>/dev/null || true
