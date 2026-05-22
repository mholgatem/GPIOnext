#!/bin/bash

#if not root user, restart script as root
if [ "$(whoami)" != "root" ]; then
	echo "Switching to root user..."
	sudo bash $0 $*
	exit 1
fi

# remove systemd service
sudo systemctl stop gpionext
sudo systemctl disable gpionext
sudo rm /lib/systemd/system/gpionext.service
sudo systemctl daemon-reload

# remove udev rule
sudo rm /etc/udev/rules.d/10-gpionext.rules

# remove custom command
sudo rm /usr/bin/gpionext

# remove install directory
echo "Removing /opt/gpionext..."
sudo rm -rf /opt/gpionext

echo "Removal Complete"

