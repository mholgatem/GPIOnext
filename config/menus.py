import time
from config.constants import *
from config import SQL
from cursesmenu import *
from cursesmenu.items import *
import curses
'''
---------------------------------------------------------
	This script handles menu navigation
	RETURNS: dictionary containing device name,
					number of buttons, number of axis
---------------------------------------------------------
'''

GOTO_MAIN = -999

def close():
	if CursesMenu.stdscr != None:
		CursesMenu().exit()

def clearPreviousMenu():
	# clear any previous menus
	if CursesMenu.stdscr != None:
		CursesMenu.stdscr.erase() 
			
			
def showMainMenu():
	global currentDevice
	clearPreviousMenu()
	currentDevice = {'name': None,
								'axisCount': 0,
								'buttons': 0}
								
	options = DEVICE_LIST + ['Clear Device']
	choice = SelectionMenu.get_selection( 
								strings = options,
								title = 'GPIOnext Config', 
								subtitle = 'Which virtual device do you want to CONFIGURE?'
								)
	try:
		currentDevice['name'] = options [ choice ]
	except IndexError: # user selected 'Exit'
		return None
	
	if currentDevice['name'] == 'Clear Device':
		return clearDevice()
	elif currentDevice['name']== 'Keyboard':
		title = 'Select the keys that you want to assign'
		return selectFromList( KEY_LIST, title )
	elif currentDevice['name'] == 'Commands':
		return currentDevice
	else:
		return getJoyAxisCount()
			
def clearDevice():
	clearPreviousMenu()
								
	options = DEVICE_LIST + ['← Return to Main Menu']
	choice = SelectionMenu.get_selection( 
								strings = options,
								title = 'CLEAR DEVICE', 
								subtitle = 'Remove configs for which device?',
								exit_option = False
								)

	currentDevice['name'] = options[choice]

	if currentDevice['name'] == '← Return to Main Menu':
		return GOTO_MAIN
	else:
		clearPreviousMenu()
		print( 'Deleting config files for {0}...'.format( currentDevice['name'] ))
		SQL.deleteDevice( currentDevice['name'] )
		time.sleep(1)
		return clearDevice()
		
def getJoyAxisCount( ):
	global currentDevice
	clearPreviousMenu()
	
	axisList = ['0','1','2','3','4','← Return to Main Menu']
	dpadCount = SelectionMenu.get_selection( 
							strings = axisList, 
							title = 'Configuring {0}'.format( currentDevice['name'] ), 
							subtitle = 'How many Dpads/Joysticks does this controller have?',
							exit_option = False
							)
	
	currentDevice['axisCount'] = dpadCount
	# if Return to Main Menu
	if dpadCount == 5:
		return GOTO_MAIN
	else:
		title = 'Select the buttons that you want to assign'
		return selectFromList( BUTTON_LIST, title)

def editCommandButton():
	global currentDevice
	cmdList = SQL.getDeviceRaw( 'Commands' )
	entries = [ '• Edit Command: {0}'.format( x['name'] ) for x in cmdList ]
	entries.insert( 0, '• Add New Command' )
	entries.append( '← Return to Main Menu' )
	
	edit = 2
	while edit == 2:
		clearPreviousMenu()
		choice = SelectionMenu.get_selection( 
								strings = entries, 
								title = 'Configuring {0}'.format( currentDevice['name'] ), 
								subtitle = 'Select a command to edit',
								exit_option = False
								)
		
		if choice == 0:
			return ( 'EDIT', {'command':'', 'pins': None, 'id': None, 'device': None, 'name': '', 'type':'COMMAND' } )
		elif  choice == len( entries ) - 1:
			return GOTO_MAIN
		clearPreviousMenu()
		edit = SelectionMenu.get_selection( 
								strings = ['Edit', 'Delete', '← Go Back' ], 
								title = 'Configuring {0}'.format( cmdList[ choice - 1 ]['name'] ), 
								subtitle = 'Edit or Delete this command?',
								exit_option = False
								)
	edit = 'EDIT' if edit == 0 else 'DELETE'	
	return ( edit, cmdList[ choice - 1 ] )
	
	
def selectFromList( currentList, title ):
	global currentDevice
	buttonNames = [ b[0] for b in currentList ]
	buttonNames.append( '← Return to Main Menu' )
	
	# returns list of buttons to configure
	choice = MultiSelect.get_selection( 
								strings = buttonNames, 
								title = title, 
								exit_option = False
								)
	# return to main menu
	if choice == [-1]:
		return GOTO_MAIN
	
	chosenButtons = [b for b in currentList if b[0] in choice]
	currentDevice['buttons'] = chosenButtons
	
	return currentDevice
