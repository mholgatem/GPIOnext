import os, sys
import sqlite3
from config import device, constants


SQL = None
sqlCursor = None

def dict_factory(cursor, row):
    d = {}
    for idx, col in enumerate(cursor.description):
        d[col[0]] = row[idx]
    return d

def getDevices( deviceNames, args):
	global SQL, sqlCursor
	deviceList = []
	query = 'SELECT * FROM GPIOnext WHERE device == (?)'
	for name in deviceNames:
		d = sqlCursor.execute( query, (name,) ).fetchall()
		if len ( d ) > 0:
			deviceList.append( device.Device( d, args ))
	return deviceList
	
def getDevice( deviceName, args ):
	global SQL, sqlCursor
	d = sqlCursor.execute( 'SELECT * FROM GPIOnext WHERE device LIKE (?)', (deviceName,) ).fetchall()
	return device.Device( d, args )

def deleteDevice( device ):
	global SQL, sqlCursor
	query = ('DELETE FROM GPIOnext '
					'where device == "{0}"').format( device )
	sqlCursor.execute( query )
	SQL.commit()
	
def getDeviceRaw( deviceName ):
	global SQL, sqlCursor
	d = sqlCursor.execute( 'SELECT * FROM GPIOnext WHERE device LIKE (?)', (deviceName,) )
	return d.fetchall()
	
def updateEntry( entryDict ):
	global SQL, sqlCursor
	query = ('INSERT or REPLACE INTO GPIOnext '
					'(id, device, name, type, command, pins) '
					'VALUES (:id, :device, :name, :type, :command, :pins)')
	sqlCursor.execute( query, entryDict )
	SQL.commit()

def deleteEntry( deleteDict ):
	global SQL, sqlCursor
	query = ('DELETE FROM GPIOnext '
					'where id=:id')
	sqlCursor.execute( query, deleteDict )
	SQL.commit()
	
def createDevice( device ):
	global SQL, sqlCursor
	query = ('INSERT INTO GPIOnext '
					'(device, name, type, command, pins) '
					'VALUES (?,?,?,?,?)')
	sqlCursor.executemany( query, device )
	SQL.commit()
	
def getDatabasePath ( defaultPath = '/home/pi/gpionext/config/' ):
	global SQL, sqlCursor
	path = os.path.realpath( defaultPath )
	if not os.path.isdir( path ):
		defaultPath = os.path.realpath( os.path.dirname(sys.argv[0]) )
		path = os.path.join( defaultPath,'config' )
	return os.path.join( path, 'config.db' )

def init():
	global SQL, sqlCursor
	DATABASE_PATH = getDatabasePath()
	SQL = sqlite3.connect( DATABASE_PATH, check_same_thread=False )
	SQL.row_factory = dict_factory
	sqlCursor = SQL.cursor()

	#Create database table
	sqlCursor.execute( 'CREATE TABLE IF NOT EXISTS GPIOnext ' 
									'(id INTEGER PRIMARY KEY AUTOINCREMENT '
									'UNIQUE, device TEXT, name TEXT, '
									'type TEXT, command TEXT, pins TEXT)')
	SQL.commit()


