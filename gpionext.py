#!/usr/bin/env python3
import argparse
import importlib
import os
import signal
import sys
import time
from config import gpio, SQL
from config.constants import *
from datetime import datetime

#------------------------------- ARGUMENTS -------------------------------------
#-------------------------------------------------------------------------------
parser = argparse.ArgumentParser(description='PiScraper')

parser.add_argument('--combo_delay', 
							metavar = '50', default = 50.0, type = float,
							help='Time in milliseconds to wait for combo buttons')
							
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

	
class GPIOnext:
		
	def __init__( self, args):
		# Watch for system signals
		for sig in [signal.SIGTERM, signal.SIGQUIT, signal.SIGINT]:
			signal.signal(sig, self.signal_handler)
		signal.signal(signal.SIGHUP, self.reload)
		
		self.args = args
		self.set_args( )
		gpio.setupGPIO( self.args )
		SQL.init()
		self.devices = SQL.getDevices( DEVICE_LIST, self.args )
		gpio.registerDevices( self.devices )
		self.main()

	def reload (self, signal, frame):
		self.DEBUG( addSeparator = True )
		self.DEBUG( "Received Reload Signal. Reloading GPIOnext!" )
		self.DEBUG( addSeparator = True )
		gpio.cleanup()
		importlib.reload( gpio )
		importlib.reload( SQL )
		gpio.setupGPIO( self.args )
		SQL.init()
		self.devices = SQL.getDevices( DEVICE_LIST, self.args )
		gpio.registerDevices( self.devices )
		#self.main()
		
	def signal_handler(self, signal, frame):
		self.DEBUG( addSeparator = True )
		self.DEBUG( addSeparator = True )
		self.DEBUG( "Shutting down. Received signal {0}".format( signal ))
		for device in self.devices:
			self.DEBUG("Closing device {0}".format( device.name ))
			device.injector.close()
		self.DEBUG("Cleanup GPIO Pins")
		gpio.cleanup()
		print()
		print ('Kaaaaahhhnn!')
		sys.exit(0)
		
	def set_args( self ):
		self.args.pins = [ int(x) for x in self.args.pins.split(',') ]
		self.args.combo_delay = self.args.combo_delay / 1000
		if self.args.debug:
			__location__ = os.path.realpath( os.path.join(os.getcwd(), os.path.dirname(__file__)) )
			self.args.log = open( os.path.join(__location__, 'logFile.txt'),'w' ) 
		
	def main( self ):
		try:
			while True:
				time.sleep( 3 )
		except KeyboardInterrupt:
			pass
	
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

if __name__ == "__main__":
		GPIOnext( args )
