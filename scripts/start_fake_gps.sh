NET="192.168.1.185:5555"
USER_ID="10"
MAPS="com.google.android.apps.maps"
LAT="51.9334"
LON="09.8597"

echo "=== ENABLE LOCATION FOR USER ==="
adb -s "$NET" shell \
  cmd location set-location-enabled true --user "$USER_ID"

echo
echo "=== GRANT MAPS LOCATION ==="
adb -s "$NET" shell \
  pm grant --user "$USER_ID" \
  "$MAPS" \
  android.permission.ACCESS_COARSE_LOCATION 2>/dev/null || true

adb -s "$NET" shell \
  pm grant --user "$USER_ID" \
  "$MAPS" \
  android.permission.ACCESS_FINE_LOCATION 2>/dev/null || true

adb -s "$NET" shell \
  appops set --user "$USER_ID" \
  "$MAPS" android:coarse_location allow

adb -s "$NET" shell \
  appops set --user "$USER_ID" \
  "$MAPS" android:fine_location allow

echo
echo "=== ENABLE MOCK LOCATION FOR SHELL ==="
adb -s "$NET" shell \
  appops set 2000 android:mock_location allow || true

adb -s "$NET" shell \
  appops set 0 android:mock_location allow || true

echo
echo "=== INSTALL TEMPORARY GPS PROVIDER ==="

adb -s "$NET" shell \
  cmd location providers set-test-provider-enabled gps false \
  2>/dev/null || true

adb -s "$NET" shell \
  cmd location providers remove-test-provider gps \
  2>/dev/null || true

adb -s "$NET" shell \
  cmd location providers add-test-provider gps \
  --requiresSatellite \
  --supportsAltitude \
  --supportsSpeed \
  --supportsBearing

adb -s "$NET" shell \
  cmd location providers set-test-provider-enabled gps true

adb -s "$NET" shell \
  cmd location providers set-test-provider-location gps \
  --location "$LAT,$LON" \
  --accuracy 5

echo
echo "=== VERIFY ==="
adb -s "$NET" shell \
  cmd location is-location-enabled --user "$USER_ID"

adb -s "$NET" shell dumpsys location |
  grep -i -E \
    'gps|last location|39\.9334|32\.8597' |
  tail -n 80