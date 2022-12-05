import time
import RPi.GPIO as GPIO
from config.constants import *

'''
	gpio.py handles all input as well as serves
	as hub for registering and calling all event
	methods
'''
#bitmask contains all pins currently pressed
bitmask = 0
changedState = False

pinChangeMethods = []
pinPressMethods = []
pinReleaseMethods = []
pins = []

def registerDevices( devices ):
	global pinPressMethods, pinReleaseMethods
	for device in devices:
		pinPressMethods.append( device.pressEvents )
		#pinReleaseMethods.append( device.releaseEvents )
			
def onPinChange( channel ):
	global pinChangeMethods, bitmask
	for method in pinChangeMethods:
		method( bitmask, channel )
		
def onPinPress( channel ):
	global pinPressMethods, bitmask
	for method in pinPressMethods:
		method( bitmask, channel )
		
def onPinRelease( channel ):
	global pinReleaseMethods, bitmask
	for method in pinReleaseMethods:
		method( bitmask, channel )

def bitmaskContains( value ):
	global bitmask
	bit = (1 << value)
	return bitmask & bit == bit

def bitmaskToList():
	global bitmask
	return [ x for x in range(41) if bitmaskContains(x) ]

class pin:
	
	def __init__( self, number, pull, args ):
		self.number = number
		self.pull = pull
		self.bit = ( 1 << number )
		try:
			if GPIO.gpio_function(self.number) == GPIO.IN:
				GPIO.setup(number, GPIO.IN, pull_up_down=pull)
				GPIO.add_event_detect(	self.number, 
										GPIO.BOTH, 
										callback = self.set_bitmask,
										bouncetime = args.debounce)
			else:
				print(f"Pin {self.number} is not set as input. Skipping.")
		except RuntimeError:
			print(f"Can't add edge detection for pin {self.number}(pin is already in use). Skipping.")
		except ValueError:
			print(f"{self.number} is an invalid pin number!")
	
	def set_bitmask( self, channel ):
		global bitmask
		global changedState
		changedState = True
		if self.button_pressed( ):
			bitmask |= self.bit #add channel
			onPinPress( channel )
		else:
			bitmask &= ~( self.bit ) #remove channel
			onPinRelease( channel )
		onPinChange( channel )
			
	def button_pressed( self ):
		time.sleep( 0.01 )
		#22 = pressed
		#LOW->0 + PULLUP->22 = 22
		#HIGH->1 + PULLDOWN->21 = 22
		return GPIO.input( self.number ) + self.pull == 22
		
def cleanup():
	global pins
	pinList = [p.number for p in pins]
	for p in pins:
		GPIO.remove_event_detect(p.number)
	GPIO.cleanup(pinList)
	
def setupGPIO( args ):
	global pins
	GPIO.setmode(GPIO.BOARD)
	GPIO.setwarnings(args.dev)
	pull = GPIO.PUD_UP 
	if args.pulldown:
		pull = GPIO.PUD_DOWN
	for pinNumber in args.pins:
		if pull == GPIO.PUD_DOWN:
			if pinNumber in (3,5):
				print("Can't set I2C pins to pulldown! Skipping...")
				continue
		pins.append( pin(pinNumber, pull, args) )
		