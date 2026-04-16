echo $0
basedir=$(dirname "$0")
cd "${basedir}/.."

cargo build --release
ssh -T webserver-4 'sudo mkdir -p /src/true_track_pingora'

rsync -avz \
  $CARGO_TARGET_DIR/release/true_track_pingora \
  certs/ \
  proxy/ \
  config.yaml scripts/true_track_pingora.service \
  webserver-4:true_track_pingora/

ssh webserver-4 -T 'sudo rm -rf /src/true_track_pingora/; sudo mv true_track_pingora /src/'
ssh webserver-4 "sudo setcap 'cap_net_bind_service=+ep' /src/true_track_pingora/true_track_pingora"
ssh webserver-4 -T 'sudo cp /src/true_track_pingora/true_track_pingora.service /etc/systemd/system/; sudo systemctl daemon-reload; sudo systemctl enable true_track_pingora; echo "Done enable pingora on webserver-4"'
ssh webserver-4 'sudo systemctl restart true_track_pingora.service'
