function fill() {
 var names = ["Now Playing", "Drive E: Audio CD", "Road trip", "Late night", "Gym"];
 for (var i = 0; i < names.length; i++) { lb1.appendItem(names[i]); lb2.appendItem(names[i]); }
 lb1.selectedItem = 1; lb2.selectedItem = 1;
}
