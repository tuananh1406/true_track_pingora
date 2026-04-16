echo $0
basedir=$(dirname "$0")
cd "${basedir}/.."

cargo build --release

PROD_SERVER=webserver-4

ssh ${PROD_SERVER} -T 'sudo apt update; sudo apt install -y build-essential rsync'
ssh -T ${PROD_SERVER} 'sudo mkdir -p /src/true_track_pingora'
rsync -avz \
  $CARGO_TARGET_DIR/release/true_track_pingora \
  certs \
  proxy \
  config.yaml scripts/true_track_pingora.service \
  ${PROD_SERVER}:true_track_pingora/

ssh ${PROD_SERVER} -T 'sudo rm -rf /src/true_track_pingora/; sudo mv true_track_pingora /src/'
ssh ${PROD_SERVER} -T 'sudo cp /src/true_track_pingora/true_track_pingora.service /etc/systemd/system/; sudo systemctl daemon-reload; sudo systemctl enable true_track_pingora; echo "Done enable pingora on ${PROD_SERVER}"'
ssh ${PROD_SERVER} -T 'sudo systemctl restart true_track_pingora.service'
ssh ${PROD_SERVER} -T 'sudo systemctl status true_track_pingora.service'
