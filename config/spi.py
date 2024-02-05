from spidev import SpiDev
import threading
import time

bus = 0
device = 0
bitmask = 0
totalPins = 8
threshold = 25

spi = SpiDev()

pinChangeMethods = []
pinPastThresholdMethods = []
pinReleaseMethods = []
pins = []
running = threading.Event()
running.set()


def poll(running):
    while running.is_set():
        for p in range(0, totalPins):
            data = read(p)
            pins[p].set_value(round((data - 512) / 2))
        time.sleep(0.05)


thread = threading.Thread(target=poll, args=(running,))


def open():
    spi.open(bus, device)
    spi.max_speed_hz = 1000000  # 1MHz
    thread.start()


def read(channel=0):
    adc = spi.xfer2([1, (8 + channel) << 4, 0])
    data = ((adc[1] & 3) << 8) + adc[2]
    return data


def close():
    spi.close()
    running.clear()
    thread.join()


def registerDevices(devices):
    global pinPastThresholdMethods, pinReleaseMethods
    for device in devices:
        pinPastThresholdMethods.append(device.pressEvents)
        # pinChangeMethods.append(device.pressEvents)
        pinReleaseMethods.append( device.releaseEvents )


def onPinChange(channel):
    global pinChangeMethods, bitmask
    for method in pinChangeMethods:
        method(bitmask, channel, mode=1)


def onPinPress(channel):
    global pinPastThresholdMethods, bitmask
    for method in pinPastThresholdMethods:
        method(bitmask, channel, mode=1)


def onPinRelease(channel):
    global pinReleaseMethods, bitmask
    for method in pinReleaseMethods:
        method(bitmask, channel, mode=1)


def bitmaskContains(value):
    global bitmask
    bit = (1 << value)
    return bitmask & bit == bit


def bitmaskToList():
    global bitmask
    return [x for x in range(totalPins) if bitmaskContains(x)]


def cleanup():
    close()


def setupSPI(args):
    global totalPins, bus, device, threshold
    totalPins = args.spi_channels
    bus = args.spi_busNumber
    device = args.spi_deviceNumber
    threshold = args.spi_axis_threshold

    for p in range(0, totalPins):
        pins.append(pin(p))

    open()


class pin:

    def __init__(self, number):
        self.number = number
        self.bit = (1 << number)
        self.value = 0
        self.pressed = False

    def set_value(self, value):
        global threshold
        
        if value < -255:
             value = -255
        elif value > 255:
            value = 255
            
        self.value = value
        if abs(value) > threshold:
            if not self.pressed:
                self.pressed = True
                self.set_bitmask(self.number)

        elif self.pressed:
            self.pressed = False
            self.value = 0
            self.set_bitmask(self.number)
        else:
            self.value = 0

    def set_bitmask(self, channel):
        global bitmask

        if self.pressed:
            bitmask |= self.bit  # add channel
            onPinPress(channel)
        else:
            bitmask &= ~(self.bit)  # remove channel
            onPinRelease(channel)
        onPinChange(channel)
        
    def __str__(self):
        return 'Channel {0} - Value {1} '.format(self.number, self.value)
