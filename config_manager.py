import os
if not os.geteuid() == 0:
	sys.exit('Script must be run as root')

import argparse
import re
import subprocess
import sys
import time
import sqlite3
import signal
import readline
from datetime import datetime
from evdev import ecodes as e
from config import gpio, menus, SQL
from config.constants import *


parser = argparse.ArgumentParser(description='GPIOnext Configuration Manager')
							
parser.add_argument('--pins', 
							metavar = '3,5,7,11', type = str,
							default = AVAILABLE_PINS_STRING,
							help='Comma delimited pin numbers to watch')

parser.add_argument('--debounce', 
							metavar='20', default = 20, type = int,
							help = 'Time in milliseconds for button debounce')

parser.add_argument('--pulldown', 
							dest='pulldown', default = False, action='store_true',
							help = 'Use PullDown resistors instead of PullUp')
							
parser.add_argument('--dev', 
							dest='dev', default = False, action='store_true',
							help='Show Warnings')

parser.add_argument('--debug',
							dest='debug', default = False, action='store_true',
							help='Print data for debugging purposes')
								
args = parser.parse_args()
	
	
def pcolor( color, string ):
	color = color.lower()
	colors = {
					'red': '\033[31m',
					'green': '\033[32m',
					'yellow': '\033[33m',
					'blue': '\033[34m',
					'fuschia': '\033[35m',
					'cyan': '\033[36m'
				}
	
	return '{0}{1}\033[0m'.format( colors[color], string )
		
class ConfigurationManager:

	def __init__( self, args ):
		self.args = args
		
		# Stop any running GPIOnext components
		subprocess.call(('systemctl', 'stop', 'gpionext'))
		time.sleep(1) #give time to stop processes
		try:
			active = subprocess.check_output(['systemctl', 'is-active', 'gpionext'])
		except:
			active = 'active'
		if 'active' in active:
			self.DEBUG('ERROR: systemctl stop gpionext has failed!')
			self.DEBUG('Please stop gpionext before running config!')
		else:
			self.DEBUG('gpionext service has been successfully stopped')

		self.DEBUG('Initializing SIGNAL HANDLERS')
		for sig in [signal.SIGTERM, signal.SIGQUIT, signal.SIGINT]:
			signal.signal(sig, self.signal_handler)
		
		gpio.pinPressMethods.append( self.setTimer )
		gpio.pinReleaseMethods.append( self.clearTimer )
		
		self.set_args()
		SQL.init()
		gpio.setupGPIO( self.args )		
		self.getControllerType()

	def DEBUG(self, msg = '', addSeparator = False):
		if self.args.debug or self.args.dev:
			if msg:
				date = datetime.fromtimestamp( time.time() )
				date = time.strftime('%Y-%m-%d %I:%M:%S%p')
				msg = '{0} {1} - {2}\n'.format( date, 'SYSTEM', msg )

			if addSeparator:
				msg += ('-' * 50) + '\n'
			if self.args.debug:
				self.args.log.write( msg )
				self.args.log.flush()
			if self.args.dev:
				print( msg )
				
	def signal_handler(self, signal, frame):
		# Reset console in case of abrupt exit
		os.system('reset')
		self.DEBUG( addSeparator = True )
		self.DEBUG( addSeparator = True )
		self.DEBUG( "Shutting down. Received signal {0}".format( signal ))
		self.DEBUG("Cleanup GPIO Pins")
		gpio.cleanup()
		print()
		print ('Kaaaaahhhnn!')
		print()
		print("Type: 'gpionext start' to run the daemon") 
		sys.exit(0)
	
	def set_args( self ):
		self.args.pins = [ int(x) for x in self.args.pins.split(',') ]
		#Use Board position numbering
		self.DEBUG('GPIO mode: BOARD')
			
	
	def getControllerType( self ):
		''' currentDevice = Dictionary with keys:
		 name, axisCount, buttonCount 
					- or -
		 currentDevice = None, Exit'''
		 
		currentDevice = menus.GOTO_MAIN
		while currentDevice == menus.GOTO_MAIN:
			currentDevice = menus.showMainMenu()
		
		# If in main menu and user selects 'Exit'
		if currentDevice == None:
			gpio.cleanup()
			print("Type: 'gpionext start' to run the daemon") 
			sys.exit(0)

		if currentDevice['name'] == 'Keyboard':
			self.configureKeyboard( currentDevice )
		elif currentDevice['name'] == 'Commands':
			self.configureCommands( )
		else:
			self.configureJoypad( currentDevice )
			
	def setTimer( self, bitmask, channel ):
		holdTimeRequired = 1.0
		self.timeout = time.time() + holdTimeRequired
		
	def clearTimer( self, bitmask, channel ):
		self.timeout = None
	
	def wait_for_pin( self, poll_time = 0.05 ):
		self.timeout = None
		while True:
			if self.timeout and self.timeout < time.time(): 
				self.timeout = None
				return gpio.bitmaskToList()
			time.sleep(poll_time)
	
	def waitForButtonRelease( self ):
		startTime = time.time()
		prompt = pcolor('cyan', 'Please release all buttons to continue')
		while gpio.bitmask:
			time.sleep(0.05)
			if prompt:
				if time.time() - startTime > 3:
					print( prompt )
					prompt = None
		
	def configureKeyboard( self, currentDevice ):
		menus.clearPreviousMenu()
		
		print( 'Configuring {0}'.format( currentDevice['name'] ))
		device = [] # Append new controls to this list
		deviceName = currentDevice['name'] # Current controller being configured

		# Second, Configure Buttons
		for button in currentDevice['buttons']:
			cmdName = button[0]
			command = button[1]
			unit = pcolor( 'cyan', cmdName)
			print( 'Press and hold GPIO pin(s) to map {0}'.format( unit ), end = ' ')
			sys.stdout.flush()
			pressed = self.wait_for_pin()
			pressed = ', '.join( map(str, pressed) )
			print( '- Pins(s):', pressed )
			self.waitForButtonRelease()
			device.append( (deviceName, cmdName, 'KEY', command, pressed) )
		
		# Save to Database
		print( 'Saving Configuration!' )
		# Delete Old Entries
		SQL.deleteDevice( deviceName )
		
		# Create New Entries
		SQL.createDevice( device )
		time.sleep(1)
		self.getControllerType()
		
	
	def getInput( self, prompt, prefill='' ):

		readline.set_startup_hook(lambda: readline.insert_text('\n' + prefill))
		try:
			return input(prompt).replace('\n','')
		finally:
			readline.set_startup_hook()
	  
	def configureCommands( self ):
		while True:
			cmd = menus.editCommandButton()
			if cmd == menus.GOTO_MAIN:
				return self.getControllerType()
			
			if cmd[0] == 'EDIT':
				self.editCommand( cmd[1] )
			elif cmd[0] == 'DELETE':
				SQL.deleteEntry( cmd[1] )
			
	def editCommand( self, cmd ):
		# User input Prompts
		promptButton = ( pcolor("fuschia", "Hold a button ") 
							+ "to configure this command" )
		promptCommand = ( pcolor("fuschia", "Enter a command ")
							+ "to map to this button: ")
		promptAdditonal = ("Map an additional command "
						+ pcolor( "cyan", "to this button? " ))
		promptName = (pcolor("fuschia", "Enter a name ")
							+ "for this command: ")
		
		# name command
		cmd['name'] = self.getInput( promptName, cmd['name'] )
		
		# Press Button to map
		print( promptButton, end = ' ')
		sys.stdout.flush()
		pressed =  self.wait_for_pin()
		cmd['pins'] = ', '.join( map(str, pressed) )
		print( '- Pin(s):', cmd['pins'] )
		self.waitForButtonRelease()
		
		# get commands
		print()
		answer = 'Y'
		commandList = []
		while 'Y' in answer.upper():
			# Enter new command
			commandList.append( self.getInput( promptCommand, cmd['command'] ) )
			cmd['command'] = ''
			# add another command?
			answer = input( promptAdditonal )
		cmd['command'] = '; '.join( commandList )
		cmd['device'] = 'Commands'
		SQL.updateEntry( cmd )
	
	def defineAxis( self, direction, dpad, deviceName, offset = 0 ):
		cmdName = '{0} {1}'.format( direction, dpad + 1 )
		colorDirection = pcolor( 'cyan', direction )
		colorDpad = pcolor( 'fuschia', dpad + 1 )
		print( 'Hold {0} on Dpad/Joystick {1}'.format( colorDirection, colorDpad), end = ' ')
		sys.stdout.flush()
		pressed =  self.wait_for_pin()
		pressed = ', '.join( map(str, pressed) )
		print( '- Pin(s):', pressed )
		self.waitForButtonRelease()
		if direction in ["DOWN", "RIGHT"]:
			value = JOYSTICK_AXIS.max
		else:
			value = JOYSTICK_AXIS.min
		command = '(e.EV_ABS, {0}, {1})'.format( dpad * 2 + offset, value )
		return deviceName, cmdName, 'AXIS', command, pressed
	
	
	def configureJoypad( self, currentDevice ):
		menus.clearPreviousMenu()
		
		print( 'Configuring {0}'.format( currentDevice['name'] ))
		device = [] # Append new controls to this list
		deviceName = currentDevice['name'] # Current controller being configured
		
		# First, Configure Joysticks
		for dpad in range( currentDevice['axisCount'] ):
			# Define Axes
			device.append( self.defineAxis("UP", dpad, deviceName, offset=1) )
			device.append( self.defineAxis("DOWN", dpad, deviceName, offset=1) )
			device.append( self.defineAxis("LEFT", dpad, deviceName) )
			device.append( self.defineAxis("RIGHT", dpad, deviceName) )

		# Second, Configure Buttons
		for button in currentDevice['buttons']:
			cmdName = button[0]
			command = button[1]
			unit = pcolor( 'cyan', cmdName)
			print( 'Hold {0}'.format( unit ), end = ' ')
			sys.stdout.flush()
			pressed = self.wait_for_pin()
			pressed = ', '.join( map(str, pressed) )
			print( '- Pins(s):', pressed )
			self.waitForButtonRelease()
			device.append( (deviceName, cmdName, 'BUTTON', command, pressed) )
		
		# Save to Database
		print( 'Saving Configuration!' )
		# Delete Old Entries
		SQL.deleteDevice( deviceName )
		
		# Create New Entries
		SQL.createDevice( device )
		time.sleep(1)
		
		self.getControllerType()
		
if __name__ == '__main__':
	ConfigurationManager(args)
