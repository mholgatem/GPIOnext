import subprocess
import time
import threading
from datetime import datetime
from evdev import UInput, AbsInfo, ecodes as e
from config.constants import *
from config import gpio

'''
---------------------------------------------------------
	Creates a complete virtual device 
	Joypad, Keyboard, or custom commands
---------------------------------------------------------
'''
class AbstractEvent:
	''' Generic input type from which 
		all other input types derive
	'''
	def __init__( self, entry ):
		self.name = entry['name']
		self.command = entry['command']
		self.isPressed = 0
		self.bitmask = 0
		self.injector = None
		self.pins = eval(entry['pins'])
		
		if type( self.pins ) == int:
			self.pins = ( self.pins, )
		for pin in self.pins:
			self.bitmask |= (1 << pin)
			
	def __str__( self ):
		className = '[' + self.__class__.__name__ + ']' 
		if self.name: 
			return ' '.join( [className, self.name] )
		return ' '.join( ['Unknown', className] )
		
	def __repr__(self):
		return self.__str__()
	
	''' Events are over-ridden based on control type '''
	def press( self ):
		pass
		
	def hold( self ):
		pass
		
	def release( self ):
		pass
		
	def bitmaskIn( self, gpioBitmask ):
		return gpioBitmask & self.bitmask == self.bitmask
		
	def waitForRelease( self ):
		while gpio.bitmask & self.bitmask == self.bitmask:
			time.sleep(0.01)
		self.release()
			
class Axis( AbstractEvent ):
	''' Joystick Axis Event '''
	def __init__( self, entry ):
		super().__init__( entry )
		self.command = eval( self.command )
		self.value = (self.command[1], JOYSTICK_AXIS)
		self.hasPressEvent = True
		self.hasHoldEvent = False
		self.hasReleaseEvent = True
		
	def press( self ):
		self.isPressed = time.time()
		self.injector.write( *self.command )
		self.injector.syn()
		self.waitForRelease()
		
	def hold( self ):
		pass
		
	def release( self ):
		if self.isPressed:
			self.isPressed = 0
			self.injector.write( self.command[0], self.command[1], 0 )
			self.injector.syn()
		
class Button( AbstractEvent ):
	''' Joystick Button Event '''
	def __init__( self, entry ):
		super().__init__( entry )
		self.command = int( self.command )
		
	def press( self ):
		self.isPressed = time.time()
		self.injector.write(e.EV_KEY, self.command, 1)
		self.injector.syn()
		self.waitForRelease()
		
		
	def release( self ):
		if self.isPressed:
			self.isPressed = 0
			self.injector.write(e.EV_KEY, self.command, 0)
			self.injector.syn()
		
class Key( AbstractEvent ):
	''' Keyboard Event '''
	def __init__( self, entry ):
		super().__init__( entry )
		self.command = int( self.command )
		self.holdTimer = threading.Timer( 0.35, self.hold )
		
	def press( self ):
		self.isPressed = time.time()
		self.injector.write(e.EV_KEY, self.command, 1)
		self.injector.syn()
		try:
			self.holdTimer.start()
		except:
			pass
		self.waitForRelease()
		
	def hold( self ):
		while self.isPressed:
			self.injector.write(e.EV_KEY, self.command, 2)
			self.injector.syn()
			time.sleep(.03)
		
	def release( self ):
		self.holdTimer.cancel()
		self.holdTimer = threading.Timer( 0.35, self.hold)
		if self.isPressed:
			self.isPressed = 0
			self.injector.write(e.EV_KEY, self.command, 0)
			self.injector.syn()
		
class Command( AbstractEvent ):
	''' Event for sending custom commands '''
	def __init__( self, entry ):
		super().__init__( entry )
		
	def press( self ):
		self.isPressed = time.time()
		for cmd in self.command.split('; '):
			subprocess.call( [cmd], executable='/bin/bash', shell=True)
	
	def release( self ):
		self.isPressed = 0
		
class Device:
	''' This class contains all info
	   and processes pertaining to a single device
	'''
	
	def __init__( self, params, args ):
		
		self.peripherals = [] # buttons / Axes / Keys / Commands
		self.name = None # Name of device
		self.args = args

		#Every event is called based on the pins it contains
		self.pinEvents = { pin:[] for pin in AVAILABLE_PINS }
		self.queue = []
		self.queueLock = threading.Lock()
		self.processing = False
		self.comboDelay = args.combo_delay
		self.processTimer = threading.Timer( self.comboDelay, self.processQueue )
		# If SQL returns any entries for device
		if params:
			self.name = params[0]['device']
			self.DEBUG( '-=New Device=-')
			capability = {
									e.EV_KEY : [],
									e.EV_ABS: []
								}
								
			# Generate peripheral types
			for entry in params:
				if entry['type'] == 'AXIS':
					inputType = Axis( entry )
					capability[ e.EV_ABS ] += [ inputType.value ]
				if entry['type'] == 'BUTTON':
					inputType = Button( entry )
					capability[ e.EV_KEY ] += [ inputType.command ]
				if entry['type'] == 'KEY':
					inputType = Key( entry )
					capability[ e.EV_KEY ] += [ inputType.command ]
				if entry['type'] == 'COMMAND':
					inputType = Command( entry )
				self.peripherals.append( inputType )
				
				for pin in inputType.pins:
					self.pinEvents[ pin ].append( inputType )
					self.pinEvents[ pin ].sort( key = lambda x: len( x.pins ), reverse = True )
					
			
			self.peripherals.sort( key = lambda x: len( x.pins ), reverse = True )
			# KEYBOARD
			if self.name == 'Keyboard':
				self.injector = UInput(name = 'GPIOnext Keyboard')
			# JOYSTICKS
			elif capability[e.EV_KEY] or capability[e.EV_ABS]:
				self.injector = UInput( capability, 
												name = 'GPIOnext ' + self.name,
												vendor = 9999, # used for udev/sdl2 rule
												product = 8888 #used for udev/sdl2 rule
												)
			# COMMANDS
			else:
				self.injector = lambda *x: None
				self.injector.syn = lambda *x: None
				self.injector.close = lambda *x: None
			
			# Allow each peripheral
			# to write it's own events
			for _ in self.peripherals:
				_.injector = self.injector
				self.DEBUG( ' '.join( ['Added Capability', str( _ )] ) )
			self.DEBUG( addSeparator = True )
	
	def __bool__( self ):
		return bool( self.peripherals )
		
	def __str__( self ):
		if self.name: 
			return self.name
		return ''
		
	def __repr__(self):
		return self.__str__()
	
	def DEBUG(self, msg = '', addSeparator = False):
		if self.args.debug or self.args.dev:
			if msg:
				date = datetime.fromtimestamp( time.time() )
				date = time.strftime('%Y-%m-%d %I:%M:%S%p')
				msg = '{0} {1} - {2}\n'.format( date, self.name, msg )

			if addSeparator:
				msg += ('-' * 50) + '\n'
			if self.args.debug:
				self.args.log.write( msg )
				self.args.log.flush()
			if self.args.dev:
				print( msg )
		
	# This event gets registered with gpio.py
	def pressEvents( self, gpioBitmask, channel ):
		for event in self.pinEvents[ channel ]:
			if event.bitmaskIn( gpioBitmask ):
				with self.queueLock:
					self.queue.append( event )
				gpioBitmask &= ~event.bitmask
		# start queue processing
		if not self.processing:
			self.processing = True
			if not self.processTimer.is_alive():
				try:
					self.processTimer.start()
				except RuntimeError:
					# Timer already started
					pass
		
	def processQueue( self ):	
		''' 
			This method processes press events as they enter the queue
		'''

		while True:
			with self.queueLock:
				if not self.queue:
					break
				currentEvent = self.queue.pop(0)

				try:
					currentBitmask = currentEvent.bitmask
					for event in self.queue:
						# check if button is part of combo press
						if currentEvent.bitmaskIn( event.bitmask ):
							break
					else:
						# run the method
						self.DEBUG( 'Press ' + currentEvent.name )
						threading.Thread( target=currentEvent.press ).start()
						
				except IndexError:
					pass	
				
		self.processTimer = threading.Timer( self.comboDelay, self.processQueue )
		self.processing = False
		