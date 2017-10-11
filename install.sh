#!/bin/bash

#get script path
SCRIPT=$(readlink -f $0)
SCRIPTPATH=`dirname $SCRIPT`
cd $SCRIPTPATH

# if not root user, restart script as root
if [ "$(whoami)" != "root" ]; then
	echo "Switching to root user..."
	sudo bash $SCRIPT $*
	exit 1
fi

# set constants
IP="$(ifconfig | grep -Eo 'inet (addr:)?([0-9]*\.){3}[0-9]*' | grep -Eo '([0-9]*\.){3}[0-9]*' | grep -v '127.0.0.1')"
NONE='\033[00m'
CYAN='\033[36m'
FUSCHIA='\033[35m'
UNDERLINE='\033[4m'

shopt -s nocasematch

if ! [[ "$1" == "-noupdate" ]]; then
	# run update
	echo -e "${CYAN}${UNDERLINE}Running Update...${NONE}"
	sudo apt-get update
	echo
fi

shopt -u nocasematch

# install dependencies
echo -e "${CYAN}${UNDERLINE}Installing Dependencies...${NONE}"
echo
sudo apt-get -y install python3 python3-pip python3-dev gcc
sudo apt-get -y install sqlite3 joystick
sudo apt-get install python3-rpi.gpio
sudo pip3 install evdev

# add gpionext.service to systemd
file1=$SCRIPTPATH"/gpionext.service"
cp $file1 /lib/systemd/system/
original='WorkingDirectory=/home/pi/gpionext'
sed -i 's#'$original'#WorkingDirectory='$SCRIPTPATH'#g' /lib/systemd/system/gpionext.service
systemctl enable gpionext

# create Udev rule for SDL2 applications
# old udev rule -> UDEV='SUBSYSTEM=="input", ATTRS{name}=="GPIOnext Keyboard", ENV{ID_INPUT_KEYBOARD}="1"'
UDEV='KERNEL=="event*", ATTRS{idVendor}=="9999", ATTRS{idProduct}=="8888", MODE:="0644"'
echo $UDEV > /etc/udev/rules.d/10-gpionext.rules

# add uinput/evdev to modules if not already there
if ! grep --quiet "uinput" /etc/modules; then echo "uinput" >> /etc/modules; fi
if ! grep --quiet "evdev" /etc/modules; then echo "evdev" >> /etc/modules; fi

# create bash custom commands
cp $SCRIPTPATH"/usr-bin-gpionext" /usr/bin/gpionext
config="CONFIG_PATH=${SCRIPTPATH}/config_manager.py"
sed -i '1s#^#'$config'\n#g' /usr/bin/gpionext
chmod 777 /usr/bin/gpionext

# remove retrogame if present
file1="/etc/rc.local"
file2="/home/pi/.profile"
if grep --quiet "retrogame" $file1 $file2; then
  echo "-----------------"
  echo -e "${CYAN}retrogame utility detected...${NONE}"
  echo -e "${FUSCHIA}Disable retrogame on startup? [y/n] (this can be undone)${NONE}"
  echo "-----------------"
  read USER_INPUT
  if [[ ! -z $(echo ${USER_INPUT} | grep -i y) ]]; then
    if grep --quiet "retrogame" $file1; then
      echo -e "${CYAN}disabling retrogame in ${file1}${NONE}"
      sed -i "/retrogame/s/^#*/: #/" $file1
      #how to uncomment: sed '/retrogame/s/^#//'
    fi
	if grep --quiet "retrogame" $file2; then
      echo -e "${CYAN}disabling retrogame in ${file2}${NONE}"
      sed -i "/retrogame/s/^#*/: #/" $file2
      #how to uncomment: sed '/retrogame/s/^#//'
    fi
  fi
fi

clear
echo -e "${CYAN}Install Complete!${NONE}"
read -p $'\e[35m\e[4mWould you like to run the configuration manager now?\e[0m [y/n]' USER_INPUT

#if yes, run gpionext config
if [[ ! -z $(echo ${USER_INPUT} | grep -i y) ]]; then
  sudo python3 $SCRIPTPATH/config_manager.py
  echo "-------------> Setup Complete!"
fi
#Start GPIOnext
systemctl daemon-reload
systemctl start gpionext


