#!/bin/bash

#get script path
SCRIPT=$(readlink -f $0)
SCRIPTPATH=`dirname $SCRIPT`
cd $SCRIPTPATH


#if not root user, restart script as root
if [ "$(whoami)" != "root" ]; then
	echo "Switching to root user..."
	sudo bash $SCRIPT
	exit 1
fi

#set constants
NONE='\033[00m'
CYAN='\033[36m'
FUSCHIA='\033[35m'
UNDERLINE='\033[4m'

echo
echo "Removing Dependencies..."
echo

#remove udev rules
rm -r /etc/udev/rules.d/10-gpionext.rules

#remove GPIOnext from systemd
systemctl stop gpionext
systemctl disable gpionext
rm /lib/systemd/system/gpionext.service

#remove bash commands
rm /usr/bin/gpionext

file1="/etc/rc.local"
file2="/home/pi/.profile"
if grep --quiet "retrogame" $file1 $file2; then
  echo "-----------------"
  echo "retrogame utility detected..."
  echo "Enable retrogame on startup? [y/n]"
  echo "-----------------"
  read USER_INPUT
  if [[ ! -z $(echo ${USER_INPUT} | grep -i y) ]]; then
    if grep --quiet "retrogame" $file1; then
      echo "enabling retrogame in $file1"
      sed -i "/retrogame/s/^: #//" $file1
    fi
	if grep --quiet "retrogame" $file2; then
      echo "enabling retrogame in $file2"
      sed -i "/retrogame/s/^: #//" $file2
    fi
  fi
fi

