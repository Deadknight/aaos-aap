NET="192.168.1.185:5555"

adb -s "$NET" shell \
  cmd location providers set-test-provider-enabled gps false \
  2>/dev/null || true

adb -s "$NET" shell \
  cmd location providers remove-test-provider gps \
  2>/dev/null || true

adb -s "$NET" shell \
  appops set 2000 android:mock_location deny || true

adb -s "$NET" shell \
  appops set 0 android:mock_location deny || true

echo "ARDA_TEMP_MOCK_GPS=CLEANED"