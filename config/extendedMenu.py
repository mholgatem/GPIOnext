import curses
from cursesmenu import CursesMenu
from cursesmenu.items import SelectionItem

'''
---------------------------------------------------------
Override cursesmenu.MenuItem.show to better suit
the menu options
---------------------------------------------------------
'''
def show( self, index ):
	return self.text


class MultiSelect(CursesMenu):
	"""
	A menu that simplifies item creation, just give it a list of strings and it builds the menu for you
	"""

	def __init__(self, strings, title=None, subtitle=None, show_exit_option=True):
		"""
		:ivar list[str] strings: The list of strings this menu should be built from
		"""
		super(MultiSelect, self).__init__(title, subtitle, show_exit_option)
		for index, item in enumerate(strings):
			self.append_item(SelectionItem(item, index, self))

	@classmethod
	def get_selection(cls, strings, title="Select an option", subtitle=None, exit_option=True, _menu=None):
		"""
		Single-method way of getting a selection out of a list of strings
		:param list[str] strings: the list of string used to build the menu
		:param list _menu: should probably only be used for testing, pass in a list and the created menu used \
		internally by the method will be appended to it
		"""
		menu = cls(strings, title, subtitle, exit_option)
		for item in menu.items:
			item.checked = False
			item.defaultText = item.text
			if item.defaultText != '← Return to Main Menu':
				item.text = '[ ] ' + item.defaultText
		if _menu is not None:
			_menu.append(menu)
		menu.show()
		menu.join()
		return menu.selected_option
	
	def draw(self):
		"""
		Redraws the menu and refreshes the screen. Should be called whenever something changes that needs to be redrawn.
		"""

		self.screen.border(0)
		if self.title is not None:
			self.screen.addstr(1, 2, self.title, curses.A_STANDOUT)
			buttonCount = "{0} Buttons Selected".format(len([ x.defaultText for x in self.items if x.checked ]))
			self.screen.addstr(1, len(self.title) + 4, "-", curses.A_BOLD)
			self.screen.addstr(1, len(self.title) + 7, buttonCount, curses.A_STANDOUT)
		if self.subtitle is not None:
			self.screen.addstr(2, 2, self.subtitle, curses.A_BOLD)
		
		instruction = ("[SPACEBAR]-Check/Uncheck Item "
							"[ENTER]-Continue")
		self.screen.addstr(3, 4, instruction, curses.A_BOLD)
		
		

		for index, item in enumerate(self.items):
			if self.current_option == index:
				text_style = self.highlight
			else:
				text_style = self.normal
			self.screen.addstr(5 + index, 4, item.show(index), text_style)

		screen_rows, screen_cols = CursesMenu.stdscr.getmaxyx()
		top_row = 0
		if 6 + len(self.items) > screen_rows:
			if screen_rows + self.current_option < 6 + len(self.items):
				top_row = self.current_option
			else:
				top_row = 6 + len(self.items) - screen_rows

		self.screen.refresh(top_row, 0, 0, 0, screen_rows - 1, screen_cols - 1)
		
	def select_many(self):
		"""
		Select multiple items
		"""
		
		if self.items[ self.current_option ].defaultText == '← Return to Main Menu':
			self.selected_option = [-1]
		else:
			self.selected_option = [ x.defaultText for x in self.items if x.checked ]
		self.selected_item.set_up()
		self.selected_item.action()
		self.selected_item.clean_up()
		self.returned_value = self.selected_item.get_return()
		self.should_exit = self.selected_item.should_exit

		if not self.should_exit:
			self.draw()
			
	def process_user_input(self):
		"""
		Gets the next single character and decides what to do with it
		"""
		user_input = self.get_input()

		go_to_max = ord("9") if len(self.items) >= 9 else ord(str(len(self.items)))

		if ord('1') <= user_input <= go_to_max:
			self.go_to(user_input - ord('0') - 1)
		elif user_input == curses.KEY_DOWN:
			self.go_down()
		elif user_input == curses.KEY_UP:
			self.go_up()
		elif user_input == ord(" "):
			if self.items[ self.current_option] .defaultText != '← Return to Main Menu':
				item = self.items[self.current_option]
				item.checked = not item.checked
				item.text = ('[X] ' if item.checked else '[ ] ') + item.defaultText
				self.items[self.current_option] = item
				self.draw()
		elif user_input in {curses.KEY_ENTER, 10, 13}:
			self.select_many()

		return user_input

	def append_string(self, string):
		self.append_item(SelectionItem(string))
