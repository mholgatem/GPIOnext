<h1>GPIOnext</h1>
<h6>A Python Based GPIO Controller</h6>This is a GPIO controller that is fully compatible with RetroPie (and PiPlay). For anyone that is familiar with Adafruit's RetroGame Utility, this is very similar. The main difference being that this is user friendly and full featured.
<h4>What's New?</h4>
<ul><li>Configuration tool to auto map buttons to keystrokes</li>
<li>web-front end to easily modify settings/will auto integrate with piplay's web frontend</li>
<li>supports button combinations for additional keystrokes</li>
<li>map multiple keystrokes/commands to a single button</li>
<li><b>It supports system commands! (you can map volume/shutdown/etc to buttons)</b></li>
</ul>
<h4>How to install</h4>in terminal type:
<pre>cd ~
git clone https://github.com/mholgatem/GPIOnext.git
bash gpionext/install.sh</pre>
That's it! The installer is still very much in the beta stage, so let me know if you have problems. But I have tested it on several clean raspbian/piplay images with no problem.

<h4>How to use</h4> After the installer runs, you will be prompted to run the configuration tool. Just follow the command prompts to set up any controls that you want. After exiting, type 'gpionext start' to run the daemon in the background
You can stop/start/run config from the command line simply by typing any of the following:
<pre>gpionext stop
gpionext start
gpionext config</pre>