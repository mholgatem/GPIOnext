from config import spi
import argparse
import time

parser = argparse.ArgumentParser(description='PiScraper')

parser.add_argument('--spi_channels', 
							metavar = '8', type = int,
							default = 2,
							help='Number of SPI channels to watch')

parser.add_argument('--spi_busNumber', 
							metavar = '0', type = int,
							default = 0,
							help='SPI Bus Number')

parser.add_argument('--spi_deviceNumber', 
							metavar = '0', type = int,
							default = 0,
							help='SPI Device Number')

parser.add_argument('--spi_axis_threshold', 
							metavar = '25', type = int,
							default = 25,
							help='SPI Axis Threshold Value (0-255)')

args = parser.parse_args()

spi.setupSPI(args)

CLEAR_LINE= '\x1b[2K'
LINE_UP = '\x1b[1A'

print("Starting SPI Test")

def main( self ):
    try:
        while True:
            for p in spi.pins:
                print(p, spi.bitmaskToList())
            time.sleep( 0.5 )
            for p in spi.pins:
                print(f"{LINE_UP}{CLEAR_LINE}", end="")

    except KeyboardInterrupt:
        spi.close()
        pass

main( None )
